## 💻 Istruzioni di Implementazione: Event System (Core Rust & Mod Runtimes)

L'obiettivo è creare un sistema di **Event Dispatcher** e un **API Adapter** per la registrazione degli handler da parte dei Mod, in conformità con i principi **Language-Agnostic** e **Zero-Copy** di Staminal.

---

### 1. Architettura Generale e Persistenza

* **Registrazione:** I Mod registrano gli handler durante l'`onAttach()`.
* **Persistenza:** Le registrazioni devono persistere fino alla chiusura dell'applicazione o all'esecuzione di `onDetach()` del Mod.
* **Struttura Dati (Core Rust):** Creare una struttura interna nel Core per archiviare gli handler. Ogni handler registrato deve memorizzare:
    * **ID del Mod** (per tracciamento e *detach*).
    * **Riferimento Richiamabile:** Per JavaScript, questo deve essere `rquickjs::Persistent<Function>`.
    * **Priorità** (`i32`).
    * **Filtri Specifici** (Protocollo, Route) per `SystemEvents::UriRequest`.
* **Dispatcher:** La funzione di *dispatch* deve eseguire la catena di handler in modo **sequenziale**, rispettando l'ordine di priorità (dal valore più piccolo al più grande).

---

### 2. API `system.register_event()`

* **Signature Esposta (JS):** `system.register_event(event_enum: SystemEvents, handler: Function, priority: i32, ...variadic_args)`
* **Implementazione Rust (Adapter):** L'implementazione di Rust deve estrarre e validare i parametri.
* **`SystemEvents`:** Implementare l'enum `SystemEvents` in Rust ed esporre un oggetto JavaScript corrispondente (mappatura dell'enum) all'API `system`.

---

### 3. Implementazione `SystemEvents::UriRequest`

#### 3.1. Filtri di Registrazione

Quando un Mod si registra per `SystemEvents::UriRequest`, i parametri aggiuntivi sono:

* **Protocollo (Protocol):** Enum `RequestUriProtocols` (`.Stam`, `.Http`, `.All` - default).
* **Route (Route):** Stringa opzionale.
    * **Regola Matching:** Il matching deve avvenire sul percorso URI (la parte dopo autorità/porta) e deve essere un **prefisso esatto**. La stringa `Route` non deve contenere lo schema o l'autorità.

#### 3.2. Oggetti Request e Response (Core Allocation)

Il Core deve preparare e mantenere due strutture dati persistenti per l'intera catena di esecuzione: `Request` e `Response`.

| Oggetto | Campo | Tipo | Descrizione / Default |
| :--- | :--- | :--- | :--- |
| **Request** | `uri` | Stringa | L'URI completo richiesto. |
| **Response** | `status` | `u16` | Codice di stato HTTP. **Default: `404`**. |
| **Response** | `handled` | `bool` | Indica se la richiesta è stata gestita. **Default: `false`**. |
| **Response** | `buffer` | `&mut [u8]` | L'interfaccia **Zero-Copy** per la scrittura. |
| **Response** | `buffer_size` | `u64` | Dimensione effettiva dei dati scritti. **Default: `0`**. |
| **Response** | `filepath` | Stringa | Path relativo di un file di risposta. **Default: Vuota** |

* **Allocazione Buffer:** La dimensione del buffer deve essere prelevata dalla configurazione del gioco (`event_buffer_size`).

#### 3.3. API di Manipolazione (Esposte al Mod)

I Mod devono accedere ai campi `Response` tramite funzioni esposte dall'Adapter Rust:

* `Response.set_status(status_code: u16)`
* `Response.set_filepath(path: String)`
* `Response.set_size(bytes_written: u64)` **(Cruciale per il Zero-Copy)**
* `Response.set_handled(is_handled: bool)`

#### 3.4. Logica di Esecuzione e Interruzione

1.  **Chiamata Sequenziale:** Eseguire l'handler del Mod.
2.  **Gestione Successo/Gestito:**
    * Se l'handler termina **senza eccezioni**, il Core controlla `Response.handled`.
    * **Se `Response.handled` è `true` (impostato dal Mod), l'esecuzione della catena deve INTERROMPERSI IMMEDIATAMENTE.**
    * *Nota per il Mod:* Se un Mod imposta `Response.status` a qualsiasi valore **NON-404** (es. `200`), deve anche impostare esplicitamente `Response.handled` a `true` per assicurare l'interruzione.
3.  **Gestione Eccezioni (Errore Script):**
    * Se l'esecuzione di un handler genera un'eccezione (errore JS, timeout, ecc.):
        * `Response.handled` **DEVE essere impostato a `true`**.
        * `Response.status` deve essere impostato a `500`.
        * `Response.buffer_size` deve essere impostato a `0`.
        * `Response.filepath` deve essere svuotata.
        * **L'esecuzione della catena deve INTERROMPERSI IMMEDIATAMENTE.**
4.  **Termine Finale:** Dopo l'interruzione (per successo o errore) o al termine naturale della catena, il Core deve inviare la risposta finale composta da `status`, la parte valida del `buffer` (fino a `buffer_size`), e `filepath`.

---

### 4. Test Iniziale

* **Scenario:** Verificare la registrazione dell'evento in `Mods-Manager` (come già inserito nell'`onAttach`).
* **Aggiungere Test:** Implementare un test unitario o di integrazione in Rust che:
    1.  Simuli la registrazione dell'handler dal Mod.
    2.  Richiami la funzione di *dispatch* per `SystemEvents::UriRequest`.
    3.  Verifichi che l'handler del Mod venga correttamente richiamato e che la logica di *dispatch* e filtro sia funzionante.
