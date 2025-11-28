# Mods Package consistency

## Task: Create a Node.js Mod Packaging Tool for Staminal Engine

**Status: IMPLEMENTED**

**Context**
A cross-platform Node.js script (ES Module `.mjs`) to package game mods into ZIP files based on manifest.json configuration. The project is a game engine workspace ("Staminal").

**File Location**
Script: `apps/server_mods_packer/mods-packer.mjs`

**Usage**
```bash
node mods-packer.mjs <input-dir> <output-dir> [--purge]
```

**Arguments:**
- `input-dir` - Directory to scan for mods (contains manifest.json files)
- `output-dir` - Directory where ZIP files will be created

**Options:**
- `--purge` - Remove obsolete ZIP files after packing

**Implementation Details**

1.  **Input & Output**
    * Requires two mandatory arguments: input directory and output directory.
    * Exits with usage help if arguments are missing.

2.  **Directory Traversal**
    * Recursively scans input directory for directories containing `manifest.json`.
    * Skips: `mod-packages`, `node_modules`, `target`, hidden directories (`.*`).
    * Handles symbolic links correctly.

3.  **Output Destination**
    * All ZIP files are placed in the specified output directory.
    * Directory is created if it doesn't exist.

4.  **Mod Naming Convention**
    * **Format:** `{id}-v{version}-{platforms}.zip`
    * **Fields:**
        * `id`: From the `name` property in manifest.json.
        * `version`: From the `version` property in manifest.json.
        * `platforms`: Derived from the `execute_on` property.
            * If string, use it directly.
            * If array, sort alphabetically and join with `+`.
    * **Examples:**
        * `js-helper-v0.1.0-client+server.zip`
        * `mods-manager-v0.1.0-server.zip`

5.  **Validation**
    * Critical error if mod id or platform contains invalid characters.
    * Allowed: `[a-zA-Z0-9_\-\.]`

6.  **Purge Feature (`--purge`)**
    * Deletes ZIP files in output directory that weren't generated in current run.

7.  **NPM Scripts** (in root package.json)
    * `npm run pack:mods` - Pack mods from `./mods` to `./mod-packages`
    * `npm run pack:mods:purge` - Pack and purge obsolete

**Technical Details**
* ES Module (`.mjs`)
* Uses `archiver` package for ZIP creation (max compression)
* Cross-platform compatible (Windows/Linux/macOS)
* Graceful error handling with detailed logging
