# Analisi Compatibilità Cross-Platform: JavaScript Runtime per Staminal Client

## 🎯 Obiettivo

Valutare le limitazioni cross-platform (Windows, macOS, Linux) delle soluzioni JavaScript proposte per l'integrazione mod nel client Staminal.

---

## 📊 Compatibilità Runtimes JavaScript

### 1. **QuickJS via `rquickjs`** ⭐ RACCOMANDATO

#### ✅ Supporto Cross-Platform

**Piattaforme supportate:**
- ✅ **Linux** (x86_64, ARM, ARM64)
- ✅ **Windows** (Windows 10+, x86_64)
- ✅ **macOS** (10.12+, x86_64, Apple Silicon M1/M2)

#### ⚠️ Limitazioni e Considerazioni

**Bindings Pre-generati:**
- `rquickjs` include bindings pre-generati per le piattaforme più comuni
- Per piattaforme non supportate dai bindings pre-generati, è necessario:
  ```toml
  rquickjs = { version = "0.6", features = ["bindgen", "classes", "properties"] }
  ```
  - Feature `bindgen` richiede:
    - `libclang` installato sul sistema (per generare bindings)
    - Aumenta i tempi di compilazione (~30-60s aggiuntivi)

**Build System:**
- Compila una libreria C (QuickJS core)
- Richiede un C compiler:
  - **Linux**: gcc/clang (di solito già presente)
  - **Windows**: MSVC (Visual Studio Build Tools) o MinGW-w64
  - **macOS**: Xcode Command Line Tools (inclusi con Xcode)

**Limitazioni funzionali:**
- Features sperimentali (es. `parallel`) potrebbero non funzionare su tutte le piattaforme
- Per il nostro use case (mods single-threaded) non è un problema

#### 📦 Dimensioni Binary

| Piattaforma | Binary Size (release) | Runtime Memory |
|-------------|----------------------|----------------|
| Linux       | ~800 KB              | ~2-5 MB        |
| Windows     | ~850 KB              | ~2-5 MB        |
| macOS       | ~900 KB              | ~2-5 MB        |

**Vantaggio:** Footprint molto ridotto, ideale per distribuzione client gaming.

#### 🔧 Setup Requisiti per Build

```bash
# Linux (Debian/Ubuntu)
sudo apt-get install build-essential libclang-dev

# macOS
xcode-select --install

# Windows
# Installare Visual Studio Build Tools 2019+ con C++ support
# Oppure: MSYS2/MinGW-w64
```

#### ✅ Conclusione rquickjs

**PRO:**
- ✅ Eccellente supporto cross-platform
- ✅ Build affidabile su Windows, macOS, Linux
- ✅ Dimensioni ridotte
- ✅ Setup build relativamente semplice

**CONTRO:**
- ⚠️ Richiede C compiler disponibile
- ⚠️ Feature `bindgen` aumenta tempi compilazione (opzionale)

**RACCOMANDAZIONE:** ✅ Ottima scelta per produzione cross-platform.

---

### 2. **V8 via `rusty_v8` / `deno_core`**

#### ✅ Supporto Cross-Platform

**Piattaforme supportate:**
- ✅ **Linux** (x86_64, ARM64)
- ✅ **Windows** (Windows 7+, x86_64)
- ✅ **macOS** (10.12+, x86_64, Apple Silicon)

#### ⚠️ Limitazioni e Considerazioni

**Build System Complesso:**
- V8 è un engine C++ molto complesso (~20MB di codice sorgente)
- `rusty_v8` integra il build system V8 in Cargo
- **Tempi di compilazione:**
  - Prima build: 10-20 minuti (compila V8 da zero)
  - Builds successive: 2-5 minuti
- Richiede:
  - Python 3 (per build scripts V8)
  - Git
  - C++ compiler moderno (MSVC 2019+, Clang 10+, GCC 9+)
  - ~2 GB spazio disco durante build
  - ~4 GB RAM per compilazione

**Binaries Pre-compilati:**
- `rusty_v8` fornisce binaries V8 pre-compilati per:
  - Linux x86_64
  - Windows x86_64
  - macOS x86_64 & ARM64
- Se binaries disponibili, build è molto più veloce (~2 min)
- In caso contrario, compila V8 da zero (lento)

**Cross-compilation:**
- Molto complessa (V8 ha toolchain specifiche per piattaforma)
- Compilare per Windows da Linux richiede setup elaborato
- Compilare per macOS da Linux praticamente impossibile senza VM

#### 📦 Dimensioni Binary

| Piattaforma | Binary Size (release) | Runtime Memory |
|-------------|----------------------|----------------|
| Linux       | ~18-25 MB            | ~10-30 MB      |
| Windows     | ~20-28 MB            | ~10-30 MB      |
| macOS       | ~22-30 MB            | ~10-30 MB      |

**Svantaggio:** Binary molto più grande di QuickJS.

#### 🔧 Setup Requisiti per Build

```bash
# Linux (Debian/Ubuntu)
sudo apt-get install python3 git build-essential

# macOS
xcode-select --install
brew install python3

# Windows
# Visual Studio 2019+ con C++ Desktop Development
# Python 3.8+
# Git
```

#### ✅ Conclusione rusty_v8/deno_core

**PRO:**
- ✅ Compatibilità JavaScript completa (ES2023+)
- ✅ Performance eccezionali
- ✅ Supporto cross-platform solido

**CONTRO:**
- ⛔ Build molto lenta (specialmente prima volta)
- ⛔ Binary size enorme (~20-30 MB)
- ⛔ Requisiti build complessi
- ⛔ Cross-compilation difficile
- ⛔ Consuma più memoria

**RACCOMANDAZIONE:** ⚠️ Solo se hai realmente bisogno delle feature ES2023+ avanzate. Per mods semplici è overkill.

---

### 3. **Boa JS Engine**

#### ✅ Supporto Cross-Platform

**Piattaforme supportate:**
- ✅ **Linux** (tutte le architetture supportate da Rust)
- ✅ **Windows** (tutte le architetture supportate da Rust)
- ✅ **macOS** (tutte le architetture supportate da Rust)

#### ⚠️ Limitazioni e Considerazioni

**100% Rust Puro:**
- Nessuna dipendenza C/C++
- Build semplice e veloce
- Cross-compilation facile (uguale a qualsiasi crate Rust)

**Maturità:**
- Supporta ~93% ECMAScript spec (test262)
- Mancano alcune feature avanzate
- Performance inferiori a QuickJS e V8
- API può cambiare (versione < 1.0)

**Compatibilità JavaScript:**
- Buon supporto ES6+ base
- Alcune feature async potrebbero avere bug
- Documentazione meno completa di alternative

#### 📦 Dimensioni Binary

| Piattaforma | Binary Size (release) | Runtime Memory |
|-------------|----------------------|----------------|
| Linux       | ~3-5 MB              | ~5-10 MB       |
| Windows     | ~3-5 MB              | ~5-10 MB       |
| macOS       | ~3-5 MB              | ~5-10 MB       |

**Nota:** Più grande di QuickJS ma molto più piccolo di V8.

#### 🔧 Setup Requisiti per Build

```bash
# Tutti i sistemi: solo Rust toolchain
rustup target add x86_64-pc-windows-gnu  # Cross-compile Windows da Linux
rustup target add x86_64-apple-darwin    # Cross-compile macOS da Linux (limitato)
```

**Vantaggio:** Nessuna dipendenza esterna, solo Rust.

#### ✅ Conclusione Boa

**PRO:**
- ✅ Build semplicissima (100% Rust)
- ✅ Cross-compilation facile
- ✅ Nessuna dipendenza C/C++
- ✅ Dimensioni ragionevoli

**CONTRO:**
- ⚠️ Performance inferiori (~2-3x più lento di QuickJS)
- ⚠️ Meno maturo (API può cambiare)
- ⚠️ Documentazione limitata
- ⚠️ Possibili bug in feature avanzate

**RACCOMANDAZIONE:** ⚠️ Valido per progetti sperimentali, ma per produzione meglio QuickJS o V8.

---

## 🏆 Raccomandazione Finale per Staminal Client

### ✅ **SCELTA CONSIGLIATA: QuickJS (rquickjs)**

**Motivazioni:**

1. **Compatibilità Cross-Platform Eccellente:**
   - Supporto solido Windows, macOS, Linux
   - Build system testato e affidabile
   - Bindings pre-generati per piattaforme comuni

2. **Requisiti Build Ragionevoli:**
   - C compiler richiesto (standard per dev environment)
   - Build veloce (~30s-1min)
   - Nessun requisito esotico

3. **Dimensioni Ottimali:**
   - ~800 KB vs ~20-30 MB di V8
   - Critico per distribuzione client gaming

4. **Performance/Features Balance:**
   - Sufficiente per UI mods e scripting
   - ES6+ base supportato
   - Performance buone per use case mods

5. **Production Ready:**
   - Usato in produzione da molti progetti
   - API stabile
   - Documentazione completa

### 📋 Configurazione Consigliata

```toml
# apps/stam_client/Cargo.toml

[dependencies]
# Per massima compatibilità cross-platform
rquickjs = { version = "0.6", features = ["classes", "properties"] }

# Se hai problemi di bindings su piattaforme esotiche:
# rquickjs = { version = "0.6", features = ["bindgen", "classes", "properties"] }
```

### 🚀 Setup Build Environment

**Linux (Debian/Ubuntu):**
```bash
sudo apt-get install build-essential
```

**macOS:**
```bash
xcode-select --install
```

**Windows:**
- Visual Studio Build Tools 2019+ con "Desktop development with C++"
- Oppure: MSYS2/MinGW-w64

### 🧪 Testing Cross-Platform

**Piano test:**
1. Build e test su Linux (development primario)
2. Cross-compile e test su Windows VM
3. Cross-compile e test su macOS VM (o GitHub Actions)

**GitHub Actions CI/CD:**
```yaml
# .github/workflows/build.yml
strategy:
  matrix:
    os: [ubuntu-latest, windows-latest, macos-latest]
runs-on: ${{ matrix.os }}
```

---

## 📚 Riferimenti

### rquickjs
- [rquickjs - crates.io](https://crates.io/crates/rquickjs/0.8.1)
- [rquickjs Documentation](https://docs.rs/rquickjs/latest/rquickjs/)
- [rquickjs on Lib.rs](https://lib.rs/crates/rquickjs)

### rusty_v8 / deno_core
- [Announcing Stable V8 Bindings for Rust](https://deno.com/blog/rusty-v8-stabilized)
- [GitHub - denoland/rusty_v8](https://github.com/denoland/rusty_v8)
- [deno_core - crates.io](https://crates.io/crates/deno_core)
- [Deno's Other Open Source Projects](https://deno.com/blog/open-source)

### Boa
- [GitHub - boa-dev/boa](https://github.com/boa-dev/boa)
- [Boa JS Official Site](https://boajs.dev/)
- [boa_engine - crates.io](https://crates.io/crates/boa_engine)
- [Boa (JavaScript engine) - Wikipedia](https://en.wikipedia.org/wiki/Boa_(JavaScript_engine))

### Cross-compilation Rust
- [A Rust cross compilation journey](https://blog.crafteo.io/2024/02/29/my-rust-cross-compilation-journey/)
- [Guide: Cross-compiling Rust from macOS to Raspberry Pi](https://sebi.io/posts/2024-05-02-guide-cross-compiling-rust-from-macos-to-raspberry-pi-2024-apple-silicon/)

---

## ✅ Conclusione

**Non ci sono limitazioni significative cross-platform per QuickJS (`rquickjs`).**

Tutti i sistemi operativi target (Windows, macOS, Linux) sono pienamente supportati con:
- Build affidabile
- Bindings pre-generati (o generabili con `bindgen`)
- Requisiti build standard (C compiler)
- Dimensioni binary ottimali

**Action Items:**
1. ✅ Usare `rquickjs` con feature `classes` e `properties`
2. ✅ Testare build su tutte le piattaforme via CI/CD
3. ✅ Documentare requisiti build per contributors
4. ✅ Fornire binary pre-compilati per release (evitare build users)
