#!/usr/bin/env node
/**
 * Mods Packer for Staminal Engine
 *
 * Packages game mods into tar.gz files based on manifest.json configuration.
 * Recursively scans input directory for directories containing manifest.json files.
 * Output tar.gz files are placed in the specified output directory.
 *
 * Usage:
 *   node mods-packer.mjs <input-dir> <output-dir> [--purge]
 *
 * Arguments:
 *   input-dir   Directory to scan for mods (contains manifest.json files)
 *   output-dir  Directory where tar.gz files will be created
 *
 * Options:
 *   --purge     Remove obsolete tar.gz files after packing
 */

import { createHash } from 'crypto';
import { createReadStream, createWriteStream, existsSync, mkdirSync, readdirSync, readFileSync, renameSync, rmSync, statSync, writeFileSync } from 'fs';
import { basename, join, relative, resolve } from 'path';
import { createGzip } from 'zlib';
import { pack } from 'tar-stream';
import { pipeline } from 'stream/promises';

// Valid characters for mod id and platform names
const VALID_CHARS_REGEX = /^[a-zA-Z0-9_\-\.]+$/;

/**
 * Parse command line arguments
 */
function parseArgs() {
    const args = process.argv.slice(2);
    let purge = false;
    const paths = [];

    for (const arg of args) {
        if (arg === '--purge') {
            purge = true;
        } else if (!arg.startsWith('-')) {
            paths.push(arg);
        }
    }

    const inputDir = paths[0] || null;
    const outputDir = paths[1] || null;

    return { purge, inputDir, outputDir };
}

/**
 * Validate that a string contains only allowed characters
 */
function validateChars(value, fieldName) {
    if (!VALID_CHARS_REGEX.test(value)) {
        throw new Error(`Invalid characters in ${fieldName}: "${value}". Allowed: letters, digits, underscore, hyphen, dot`);
    }
}

/**
 * Parse manifest.json and generate archive filename
 */
function parseManifest(manifestPath) {
    const content = readFileSync(manifestPath, 'utf-8');
    let manifest;

    try {
        manifest = JSON.parse(content);
    } catch (e) {
        throw new Error(`Malformed JSON in ${manifestPath}: ${e.message}`);
    }

    const { name, version, execute_on } = manifest;

    if (!name) {
        throw new Error(`Missing 'name' in ${manifestPath}`);
    }
    if (!version) {
        throw new Error(`Missing 'version' in ${manifestPath}`);
    }
    if (!execute_on) {
        throw new Error(`Missing 'execute_on' in ${manifestPath}`);
    }

    // Validate name (id)
    validateChars(name, 'name');

    // Process platforms
    let platforms;
    if (typeof execute_on === 'string') {
        validateChars(execute_on, 'execute_on');
        platforms = execute_on;
    } else if (Array.isArray(execute_on)) {
        for (const platform of execute_on) {
            validateChars(platform, 'execute_on');
        }
        platforms = [...execute_on].sort().join('+');
    } else {
        throw new Error(`Invalid 'execute_on' type in ${manifestPath}: expected string or array`);
    }

    const archiveName = `${name}-v${version}-${platforms}.tar.gz`;

    return { id: name, version, platforms, archiveName, manifest };
}

/**
 * Recursively find all mod roots (directories containing manifest.json)
 * Skips mod-packages and node_modules directories
 */
function findModRoots(dir, results = []) {
    if (!existsSync(dir)) {
        return results;
    }

    const entries = readdirSync(dir, { withFileTypes: true });

    for (const entry of entries) {
        // Skip non-directories and special directories
        if (!entry.isDirectory() && !entry.isSymbolicLink()) continue;
        if (entry.name === 'mod-packages') continue;
        if (entry.name === 'node_modules') continue;
        if (entry.name === 'target') continue;
        if (entry.name.startsWith('.')) continue;

        const subDir = join(dir, entry.name);

        // Handle symlinks - resolve and check if directory
        let isDir = entry.isDirectory();
        if (entry.isSymbolicLink()) {
            try {
                const stat = statSync(subDir);
                isDir = stat.isDirectory();
            } catch {
                continue; // Skip broken symlinks
            }
        }

        if (!isDir) continue;

        const manifestPath = join(subDir, 'manifest.json');

        if (existsSync(manifestPath)) {
            // Found a mod root
            results.push({ modDir: subDir, manifestPath });
            // Don't recurse into mod directories (manifest found = mod root)
        } else {
            // Continue searching in subdirectories
            findModRoots(subDir, results);
        }
    }

    return results;
}

/**
 * Recursively get all files in a directory
 * Returns array of { relativePath, absolutePath, size } sorted by relativePath
 */
function getAllFiles(dir, baseDir = dir, results = []) {
    const entries = readdirSync(dir, { withFileTypes: true });

    for (const entry of entries) {
        const absolutePath = join(dir, entry.name);
        const relativePath = relative(baseDir, absolutePath);

        if (entry.isDirectory()) {
            getAllFiles(absolutePath, baseDir, results);
        } else if (entry.isFile()) {
            const stat = statSync(absolutePath);
            results.push({ relativePath, absolutePath, size: stat.size });
        }
    }

    return results.sort((a, b) => a.relativePath.localeCompare(b.relativePath));
}

/**
 * Calculate SHA512 hash based on file paths and modification dates
 * This creates a fast hash for change detection based on:
 * - All file paths (sorted)
 * - All file modification dates in ISO format
 */
function calculateDateSha512(modDir) {
    const files = getAllFiles(modDir);

    // Build a string with all file paths and their modification dates
    const lines = [];
    for (const { relativePath, absolutePath } of files) {
        const stat = statSync(absolutePath);
        const mtime = stat.mtime.toISOString();
        lines.push(`${relativePath}\t${mtime}`);
    }

    // Join all lines and calculate SHA512
    const content = lines.join('\n');
    return createHash('sha512').update(content).digest('hex');
}

/**
 * Create a tar.gz archive of a mod directory (streaming version for large files)
 * Returns { archiveBytes, uncompressedBytes }
 */
async function createModArchive(modDir, outputPath) {
    const files = getAllFiles(modDir);
    const tarPack = pack();

    // Calculate uncompressed size
    const uncompressedBytes = files.reduce((total, file) => total + file.size, 0);

    // Create output stream with gzip compression
    const output = createWriteStream(outputPath);
    const gzip = createGzip({ level: 9 });

    // Start the pipeline
    const pipelinePromise = pipeline(tarPack, gzip, output);

    // Add all files to the tar archive using streaming
    for (const { relativePath, absolutePath, size } of files) {
        const stat = statSync(absolutePath);

        // Create entry stream for this file
        const entry = tarPack.entry({
            name: relativePath,
            size: size,
            mode: stat.mode,
            mtime: stat.mtime
        });

        // Stream the file content into the tar entry
        await new Promise((resolve, reject) => {
            const fileStream = createReadStream(absolutePath);
            fileStream.on('data', (chunk) => entry.write(chunk));
            fileStream.on('end', () => {
                entry.end();
                resolve();
            });
            fileStream.on('error', reject);
        });
    }

    // Finalize the tar archive
    tarPack.finalize();

    // Wait for the pipeline to complete
    await pipelinePromise;

    // Return archive size and uncompressed size
    const archiveStat = statSync(outputPath);
    return { archiveBytes: archiveStat.size, uncompressedBytes };
}

/**
 * Purge obsolete archive files from output directory
 */
function purgeObsolete(outputDir, validArchives) {
    if (!existsSync(outputDir)) return [];

    const purged = [];
    const entries = readdirSync(outputDir, { withFileTypes: true });

    for (const entry of entries) {
        if (!entry.isFile()) continue;
        if (!entry.name.endsWith('.tar.gz')) continue;

        if (!validArchives.includes(entry.name)) {
            const filePath = join(outputDir, entry.name);
            try {
                rmSync(filePath);
                console.log(`  Purged: ${entry.name}`);
                purged.push(entry.name);
            } catch (e) {
                console.error(`  Failed to purge ${entry.name}: ${e.message}`);
            }
        }
    }

    return purged;
}

/**
 * Calculate SHA512 hash of a file (for archive files)
 */
async function calculateFileSha512(filePath) {
    return new Promise((resolve, reject) => {
        const hash = createHash('sha512');
        const stream = createReadStream(filePath);
        stream.on('data', (data) => hash.update(data));
        stream.on('end', () => resolve(hash.digest('hex')));
        stream.on('error', reject);
    });
}

/**
 * Format bytes to human readable string
 */
function formatBytes(bytes) {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

/**
 * Print usage help
 */
function printUsage() {
    console.error('Mods Packer for Staminal Engine');
    console.error('');
    console.error('Usage: node mods-packer.mjs <input-dir> <output-dir> [--purge]');
    console.error('');
    console.error('Arguments:');
    console.error('  input-dir   Directory to scan for mods (contains manifest.json files)');
    console.error('  output-dir  Directory where tar.gz files will be created');
    console.error('');
    console.error('Options:');
    console.error('  --purge     Remove obsolete tar.gz files after packing');
    console.error('');
    console.error('Example:');
    console.error('  node mods-packer.mjs ./mods ./mod-packages');
    console.error('  node mods-packer.mjs ./mods ./mod-packages --purge');
}

/**
 * Load existing mod-packages.json if it exists
 * Returns a map of archive path -> package info for quick lookup
 * Uses archive path as key since the same mod ID can have different versions for client/server
 */
function loadExistingPackages(outputDir) {
    const modPackagesPath = join(outputDir, 'mod-packages.json');
    const existingPackages = new Map();

    if (existsSync(modPackagesPath)) {
        try {
            const content = readFileSync(modPackagesPath, 'utf-8');
            const data = JSON.parse(content);

            // Index by archive path for quick lookup (handles same mod ID with different platforms)
            for (const pkg of data.client || []) {
                existingPackages.set(pkg.path, pkg);
            }
            for (const pkg of data.server || []) {
                existingPackages.set(pkg.path, pkg);
            }
        } catch (e) {
            console.warn(`Warning: Could not read existing mod-packages.json: ${e.message}`);
        }
    }

    return existingPackages;
}

/**
 * Main entry point
 */
async function main() {
    const { purge, inputDir, outputDir } = parseArgs();

    if (!inputDir || !outputDir) {
        printUsage();
        process.exit(1);
    }

    const resolvedInput = resolve(inputDir);
    const resolvedOutput = resolve(outputDir);

    if (!existsSync(resolvedInput)) {
        console.error(`Error: Input directory does not exist: ${resolvedInput}`);
        process.exit(1);
    }

    console.log(`Mods Packer - Staminal Engine`);
    console.log(`Input:  ${resolvedInput}`);
    console.log(`Output: ${resolvedOutput}`);
    console.log(`Purge:  ${purge ? 'enabled' : 'disabled'}`);
    console.log('');

    // Ensure output directory exists
    if (!existsSync(resolvedOutput)) {
        mkdirSync(resolvedOutput, { recursive: true });
        console.log(`Created: ${resolvedOutput}`);
    }

    // Load existing packages for comparison
    const existingPackages = loadExistingPackages(resolvedOutput);

    // Find all mod roots recursively
    console.log('Scanning for mods...');
    const modRoots = findModRoots(resolvedInput);
    console.log(`Found ${modRoots.length} mod(s)`);
    console.log('');

    const packed = [];
    const skipped = [];
    const errors = [];
    // Collect mod package info grouped by platform
    const modPackages = {
        client: [],
        server: []
    };

    for (const { modDir, manifestPath } of modRoots) {
        try {
            const { id, version, platforms, archiveName, manifest } = parseManifest(manifestPath);
            const outputPath = join(resolvedOutput, archiveName);

            const relPath = relative(resolvedInput, modDir);

            // Calculate date-based SHA512 (hash of file paths + modification dates for fast change detection)
            const date_sha512 = calculateDateSha512(modDir);

            // Check if we need to repack (use archiveName as key since same mod ID can have different platforms)
            const existingPkg = existingPackages.get(archiveName);
            const needsRepack = !existingPkg ||
                existingPkg.date_sha512 !== date_sha512 ||
                existingPkg.path !== archiveName ||
                !existsSync(outputPath);

            let archive_sha512;
            let archive_bytes;
            let uncompressed_bytes;

            if (needsRepack) {
                console.log(`Packing: ${id} v${version} (${platforms})`);
                console.log(`  From: ${relPath}`);

                // Create archive with _temp suffix first, then rename when complete
                const tempPath = outputPath + '_temp';
                const { archiveBytes, uncompressedBytes } = await createModArchive(modDir, tempPath);

                // Calculate archive file SHA512 from temp file
                archive_sha512 = await calculateFileSha512(tempPath);
                archive_bytes = archiveBytes;
                uncompressed_bytes = uncompressedBytes;

                // Atomically replace the old archive with the new one
                renameSync(tempPath, outputPath);
                console.log(`  -> ${archiveName} (${formatBytes(archiveBytes)}, uncompressed: ${formatBytes(uncompressedBytes)})`);

                packed.push(archiveName);
            } else {
                console.log(`Skipping: ${id} v${version} (${platforms}) - unchanged`);

                // Use existing values
                archive_sha512 = existingPkg.archive_sha512;
                archive_bytes = existingPkg.archive_bytes;
                uncompressed_bytes = existingPkg.uncompressed_bytes;

                skipped.push(archiveName);
            }

            // Create package info
            const packageInfo = {
                id,
                manifest,
                date_sha512,        // Hash of file paths + dates (for fast change detection)
                archive_sha512,     // Hash of archive file (for integrity verification)
                archive_bytes,      // Size of compressed archive in bytes
                uncompressed_bytes, // Sum of all uncompressed file sizes in bytes
                path: archiveName
            };

            // Add to appropriate platform lists
            const platformList = platforms.split('+');
            if (platformList.includes('client')) {
                modPackages.client.push(packageInfo);
            }
            if (platformList.includes('server')) {
                modPackages.server.push(packageInfo);
            }
        } catch (e) {
            const modName = basename(modDir);
            console.error(`ERROR [${modName}]: ${e.message}`);
            errors.push({ mod: modName, error: e.message });
        }
    }

    console.log('');

    // Purge obsolete files if requested
    if (purge) {
        console.log('Purging obsolete packages...');
        const allArchives = [...packed, ...skipped];
        const purged = purgeObsolete(resolvedOutput, allArchives);
        console.log(`Purged: ${purged.length} file(s)`);
        console.log('');
    }

    // Write mod-packages.json
    const modPackagesPath = join(resolvedOutput, 'mod-packages.json');
    writeFileSync(modPackagesPath, JSON.stringify(modPackages, null, 2), 'utf-8');
    console.log(`Generated: mod-packages.json`);
    console.log(`  Client mods: ${modPackages.client.length}`);
    console.log(`  Server mods: ${modPackages.server.length}`);
    console.log('');

    // Summary
    console.log('Summary:');
    console.log(`  Packed:  ${packed.length} mod(s)`);
    console.log(`  Skipped: ${skipped.length} mod(s) (unchanged)`);
    console.log(`  Errors:  ${errors.length}`);

    if (errors.length > 0) {
        process.exit(1);
    }
}

main().catch((e) => {
    console.error(`Fatal error: ${e.message}`);
    process.exit(1);
});
