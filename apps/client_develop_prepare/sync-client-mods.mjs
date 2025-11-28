#!/usr/bin/env node
/**
 * Sync Client Mods Script
 *
 * This script synchronizes client mods from a source directory to a destination directory.
 * It reads mod manifests and only copies mods that have "execute_on" set to "client" or
 * an array containing "client".
 *
 * Usage: node sync-client-mods.mjs <input-dir> <output-dir>
 *
 * - Mods in output that don't exist in input (client mods only) will be deleted
 * - Only client mods are processed (based on manifest.json execute_on field)
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
 * Find and read manifest.json from a mod directory
 * Checks both root and client/ subdirectory
 * @param {string} modDir - Path to the mod directory
 * @returns {object|null} - Parsed manifest or null if not found
 */
function readModManifest(modDir) {
    // Check client/ subdirectory first
    const clientManifestPath = path.join(modDir, 'client', 'manifest.json');
    if (fs.existsSync(clientManifestPath)) {
        try {
            return JSON.parse(fs.readFileSync(clientManifestPath, 'utf8'));
        } catch (e) {
            console.error(`  Error reading ${clientManifestPath}: ${e.message}`);
            return null;
        }
    }

    // Check root manifest
    const rootManifestPath = path.join(modDir, 'manifest.json');
    if (fs.existsSync(rootManifestPath)) {
        try {
            return JSON.parse(fs.readFileSync(rootManifestPath, 'utf8'));
        } catch (e) {
            console.error(`  Error reading ${rootManifestPath}: ${e.message}`);
            return null;
        }
    }

    return null;
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
 * Get list of client mod IDs from a directory
 * @param {string} modsDir - Directory containing mods
 * @returns {Set<string>} - Set of client mod IDs
 */
function getClientModIds(modsDir) {
    const clientMods = new Set();

    if (!fs.existsSync(modsDir)) {
        return clientMods;
    }

    const entries = fs.readdirSync(modsDir, { withFileTypes: true });

    for (const entry of entries) {
        if (!entry.isDirectory()) continue;

        const modDir = path.join(modsDir, entry.name);
        const manifest = readModManifest(modDir);

        if (manifest && isClientMod(manifest)) {
            clientMods.add(entry.name);
        }
    }

    return clientMods;
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

    // Create output directory if it doesn't exist
    fs.mkdirSync(outputDir, { recursive: true });

    // Get client mods from input
    const inputClientMods = getClientModIds(inputDir);
    console.log(`Found ${inputClientMods.size} client mod(s) in input:`);
    for (const modId of inputClientMods) {
        console.log(`  - ${modId}`);
    }
    console.log();

    // Get client mods from output (to know which ones to delete)
    const outputClientMods = getClientModIds(outputDir);

    // Delete client mods from output that are not in input
    const modsToDelete = [...outputClientMods].filter(modId => !inputClientMods.has(modId));
    if (modsToDelete.length > 0) {
        console.log(`Deleting ${modsToDelete.length} mod(s) not present in input:`);
        for (const modId of modsToDelete) {
            const modPath = path.join(outputDir, modId);
            console.log(`  - Deleting: ${modId}`);
            deleteDirRecursive(modPath);
        }
        console.log();
    }

    // Copy/update client mods from input to output
    if (inputClientMods.size > 0) {
        console.log(`Copying ${inputClientMods.size} client mod(s):`);
        for (const modId of inputClientMods) {
            const srcPath = path.join(inputDir, modId);
            const destPath = path.join(outputDir, modId);

            console.log(`  - Copying: ${modId}`);

            // Delete existing mod directory first (to ensure clean copy)
            deleteDirRecursive(destPath);

            // Copy the mod
            copyDirRecursive(srcPath, destPath);
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
    console.error('  input-dir   Source directory containing mods');
    console.error('  output-dir  Destination directory for client mods');
    process.exit(1);
}

const [inputDir, outputDir] = args.map(p => path.resolve(p));

syncClientMods(inputDir, outputDir);
