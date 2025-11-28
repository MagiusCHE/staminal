#!/usr/bin/env node
/**
 * Sync Client Mods Script
 *
 * This script synchronizes client mods from a source directory to a destination directory.
 * It recursively searches for manifest.json files and copies mods that have "execute_on"
 * set to "client" or an array containing "client".
 *
 * Usage: node sync-client-mods.mjs <input-dir> <output-dir>
 *
 * - Searches recursively for all manifest.json files in input-dir
 * - For each client mod found, copies the manifest's directory to output-dir/<mod-id>/
 * - Mods in output that don't exist in input (client mods only) will be deleted
 */

import fs from 'fs';
import path from 'path';

/**
 * Check if a mod is a client mod based on its manifest
 * @param {object} manifest - The parsed manifest.json
 * @returns {boolean} - True if the mod executes on client
 */
function isClientMod(manifest) {
    const executeOn = manifest.execute_on;
    if (!executeOn) return false;

    if (typeof executeOn === 'string') {
        return executeOn === 'client';
    }

    if (Array.isArray(executeOn)) {
        return executeOn.includes('client');
    }

    return false;
}

/**
 * Recursively find all manifest.json files in a directory
 * @param {string} dir - Directory to search
 * @param {Array} results - Array to accumulate results
 * @returns {Array<{manifestPath: string, manifest: object, modDir: string}>}
 */
function findManifests(dir, results = []) {
    if (!fs.existsSync(dir)) {
        return results;
    }

    const entries = fs.readdirSync(dir, { withFileTypes: true });

    for (const entry of entries) {
        const fullPath = path.join(dir, entry.name);

        if (entry.isDirectory()) {
            // Recurse into subdirectories
            findManifests(fullPath, results);
        } else if (entry.name === 'manifest.json') {
            // Found a manifest.json
            try {
                const manifest = JSON.parse(fs.readFileSync(fullPath, 'utf8'));
                results.push({
                    manifestPath: fullPath,
                    manifest,
                    modDir: dir // The directory containing the manifest
                });
            } catch (e) {
                console.error(`  Error reading ${fullPath}: ${e.message}`);
            }
        }
    }

    return results;
}

/**
 * Recursively copy a directory
 * @param {string} src - Source directory
 * @param {string} dest - Destination directory
 */
function copyDirRecursive(src, dest) {
    fs.mkdirSync(dest, { recursive: true });

    const entries = fs.readdirSync(src, { withFileTypes: true });

    for (const entry of entries) {
        const srcPath = path.join(src, entry.name);
        const destPath = path.join(dest, entry.name);

        if (entry.isDirectory()) {
            copyDirRecursive(srcPath, destPath);
        } else {
            fs.copyFileSync(srcPath, destPath);
        }
    }
}

/**
 * Recursively delete a directory
 * @param {string} dirPath - Directory to delete
 */
function deleteDirRecursive(dirPath) {
    if (fs.existsSync(dirPath)) {
        fs.rmSync(dirPath, { recursive: true, force: true });
    }
}

/**
 * Get list of existing mod directories in output
 * @param {string} modsDir - Directory containing mods
 * @returns {Set<string>} - Set of mod directory names
 */
function getExistingModIds(modsDir) {
    const modIds = new Set();

    if (!fs.existsSync(modsDir)) {
        return modIds;
    }

    const entries = fs.readdirSync(modsDir, { withFileTypes: true });

    for (const entry of entries) {
        if (entry.isDirectory()) {
            modIds.add(entry.name);
        }
    }

    return modIds;
}

/**
 * Main sync function
 * @param {string} inputDir - Source mods directory
 * @param {string} outputDir - Destination mods directory
 */
function syncClientMods(inputDir, outputDir) {
    console.log(`Syncing client mods:`);
    console.log(`  Input:  ${inputDir}`);
    console.log(`  Output: ${outputDir}`);
    console.log();

    // Validate input directory exists
    if (!fs.existsSync(inputDir)) {
        console.error(`Error: Input directory does not exist: ${inputDir}`);
        process.exit(1);
    }

    // Get existing mods in output directory FIRST
    const existingModIds = getExistingModIds(outputDir);
    if (existingModIds.size === 0) {
        console.log(`No existing mods in output directory. Nothing to sync.`);
        return;
    }
    console.log(`Found ${existingModIds.size} existing mod(s) in output:`);
    for (const modId of existingModIds) {
        console.log(`  - ${modId}`);
    }
    console.log();

    // Find all manifest.json files recursively in input
    console.log(`Searching for manifest.json files in input...`);
    const allManifests = findManifests(inputDir);

    // Filter to only client mods and build map
    const clientModsMap = new Map();
    for (const mod of allManifests) {
        if (!isClientMod(mod.manifest)) continue;

        const modId = mod.manifest.name;
        if (!modId) continue;

        clientModsMap.set(modId, mod);
    }
    console.log(`Found ${clientModsMap.size} client mod(s) in input`);
    console.log();

    // Process existing mods in output
    const modsToUpdate = [];
    const modsToDelete = [];

    for (const modId of existingModIds) {
        if (clientModsMap.has(modId)) {
            modsToUpdate.push(modId);
        } else {
            modsToDelete.push(modId);
        }
    }

    // Delete mods from output that are not in input anymore
    if (modsToDelete.length > 0) {
        console.log(`Deleting ${modsToDelete.length} mod(s) no longer in input:`);
        for (const modId of modsToDelete) {
            const modPath = path.join(outputDir, modId);
            console.log(`  - Deleting: ${modId}`);
            deleteDirRecursive(modPath);
        }
        console.log();
    }

    // Update existing mods from input
    if (modsToUpdate.length > 0) {
        console.log(`Updating ${modsToUpdate.length} mod(s):`);
        for (const modId of modsToUpdate) {
            const mod = clientModsMap.get(modId);
            const destPath = path.join(outputDir, modId);

            console.log(`  - Updating: ${modId} (from ${path.relative(inputDir, mod.modDir)})`);

            // Delete existing mod directory first (to ensure clean copy)
            deleteDirRecursive(destPath);

            // Copy the mod directory (the one containing manifest.json)
            copyDirRecursive(mod.modDir, destPath);
        }
        console.log();
    }

    console.log('Sync completed successfully.');
}

// Main entry point
const args = process.argv.slice(2);

if (args.length !== 2) {
    console.error('Usage: node sync-client-mods.mjs <input-dir> <output-dir>');
    console.error();
    console.error('Arguments:');
    console.error('  input-dir   Source directory to search for mods (recursive)');
    console.error('  output-dir  Destination directory for client mods');
    process.exit(1);
}

const [inputDir, outputDir] = args.map(p => path.resolve(p));

syncClientMods(inputDir, outputDir);
