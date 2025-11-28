#!/usr/bin/env node
/**
 * Mods Packer for Staminal Engine
 *
 * Packages game mods into ZIP files based on manifest.json configuration.
 * Recursively scans input directory for directories containing manifest.json files.
 * Output ZIP files are placed in the specified output directory.
 *
 * Usage:
 *   node mods-packer.mjs <input-dir> <output-dir> [--purge]
 *
 * Arguments:
 *   input-dir   Directory to scan for mods (contains manifest.json files)
 *   output-dir  Directory where ZIP files will be created
 *
 * Options:
 *   --purge     Remove obsolete ZIP files after packing
 */

import { createHash } from 'crypto';
import { createReadStream, createWriteStream, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'fs';
import { basename, join, relative, resolve } from 'path';
import archiver from 'archiver';

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
 * Parse manifest.json and generate ZIP filename
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

    const zipName = `${name}-v${version}-${platforms}.zip`;

    return { id: name, version, platforms, zipName, manifest };
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
 * Create a ZIP archive of a mod directory
 */
async function createModZip(modDir, outputPath) {
    return new Promise((resolve, reject) => {
        const output = createWriteStream(outputPath);
        const archive = archiver('zip', { zlib: { level: 9 } });

        output.on('close', () => resolve(archive.pointer()));
        output.on('error', reject);
        archive.on('error', reject);
        archive.on('warning', (err) => {
            if (err.code !== 'ENOENT') {
                reject(err);
            }
        });

        archive.pipe(output);

        // Add all files from mod directory, placing them at root of ZIP
        archive.directory(modDir, false);

        archive.finalize();
    });
}

/**
 * Purge obsolete ZIP files from output directory
 */
function purgeObsolete(outputDir, validZips) {
    if (!existsSync(outputDir)) return [];

    const purged = [];
    const entries = readdirSync(outputDir, { withFileTypes: true });

    for (const entry of entries) {
        if (!entry.isFile()) continue;
        if (!entry.name.endsWith('.zip')) continue;

        if (!validZips.includes(entry.name)) {
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
 * Calculate SHA512 hash of a file
 */
async function calculateSha512(filePath) {
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
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
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
    console.error('  output-dir  Directory where ZIP files will be created');
    console.error('');
    console.error('Options:');
    console.error('  --purge     Remove obsolete ZIP files after packing');
    console.error('');
    console.error('Example:');
    console.error('  node mods-packer.mjs ./mods ./mod-packages');
    console.error('  node mods-packer.mjs ./mods ./mod-packages --purge');
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

    // Find all mod roots recursively
    console.log('Scanning for mods...');
    const modRoots = findModRoots(resolvedInput);
    console.log(`Found ${modRoots.length} mod(s)`);
    console.log('');

    const packed = [];
    const errors = [];
    // Collect mod package info grouped by platform
    const modPackages = {
        client: [],
        server: []
    };

    for (const { modDir, manifestPath } of modRoots) {
        try {
            const { id, version, platforms, zipName, manifest } = parseManifest(manifestPath);
            const outputPath = join(resolvedOutput, zipName);

            const relPath = relative(resolvedInput, modDir);
            console.log(`Packing: ${id} v${version} (${platforms})`);
            console.log(`  From: ${relPath}`);

            const size = await createModZip(modDir, outputPath);
            console.log(`  -> ${zipName} (${formatBytes(size)})`);

            // Calculate SHA512 hash
            const sha512 = await calculateSha512(outputPath);

            // Create package info
            const packageInfo = {
                id,
                manifest,
                sha512,
                path: zipName
            };

            // Add to appropriate platform lists
            const platformList = platforms.split('+');
            if (platformList.includes('client')) {
                modPackages.client.push(packageInfo);
            }
            if (platformList.includes('server')) {
                modPackages.server.push(packageInfo);
            }

            packed.push(zipName);
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
        const purged = purgeObsolete(resolvedOutput, packed);
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
    console.log(`  Packed: ${packed.length} mod(s)`);
    console.log(`  Errors: ${errors.length}`);

    if (errors.length > 0) {
        process.exit(1);
    }
}

main().catch((e) => {
    console.error(`Fatal error: ${e.message}`);
    process.exit(1);
});
