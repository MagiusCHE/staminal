## 🌐 Istruzioni di Implementazione: API `network.download`

L'obiettivo è implementare l'API `network.download` esposta ai Mod (sia Client che Server) e creare il meccanismo di comunicazione **one-shot** basato sul protocollo proprietario `stam://` e sull'integrazione con l'Event System per le richieste **`RequestUri`**.

### 1. Centralizzazione e Interfaccia

* **API Centralizzata:** L'oggetto **"network"** e il metodo **"download"** devono esistere sia per il client che per il server. Centralizzare il codice di parsing dell'URI e la logica di gestione degli schemi URI in un modulo comune (`stam_protocol`).
* **Signature (JS Mod):** `network.download(uri: string): Promise<{status: u16, buffer: Uint8Array | null, file_name: string | null, file_content: Uint8Array | null}>`

---

### 2. Implementazione Client (`Client`)

Il metodo `download(uri)` deve analizzare l'URI e gestire il protocollo.

#### A. Gestione Protocollo `stam://`

Se il protocollo è `stam://` (Download tramite *PrimalClient* ad un Server):

1.  **Parsing URI:**
    * Analizzare l'URI per estrarre: IP, Porta, Username (opzionale), Password (opzionale), e la **Route** (il percorso dopo l'autorità `ip:porta`).
2.  **Connessione:** Connettersi al Server (`ip:porta`) con una **nuova socket one-shot** (usare il connettore *PrimalClient*).
3.  **Flusso di Comunicazione:**
    * Attendere il **Welcome Message** dal server.
    * Inviare un messaggio **`PrimalMessage::Intent`** con **`IntentType::RequestUri`** e i seguenti argomenti (payload):
        * `client_version` (String)
        * `username` (String, se presente nell'URI)
        * `password_hash` (String, SHA-512) (se presente nell'URI)
        * `game_id` (String, il gioco attivo su cui è configurato il client e che esso ha ricevuto dal server durante il "PrimalLogin").
        * `uri` (String, la richiesta originale **epurata della parte sulle credenziali** per la sicurezza).
4.  **Attesa Risposta:** Attendere un singolo messaggio di risposta dal Server.
    * La risposta deve contenere i seguenti parametri serializzati:
        * `status` (`u16`).
        * `buffer` (Array di `u8` se presente).
        * se `Response.filepath` sul Server era valorizzato
          * `file_name` (String). 
          * `file_size` in bytes (ulong)
          * `file_content` (Array di byte in chunk). La grandezza del chunk deve essere dichiarata nel file di configurazione del server e come fallback è pari a 500kb (request_uri_chunk_size).
5.  **Chiusura:** Chiudere la socket immediatamente dopo aver ricevuto tutta la risposta (tutto il file).

#### B. Gestione Altri Protocolli (`http://`, `https://`)

* **TODO:** L'implementazione Client deve concentrarsi solo sul protocollo `stam://`. **Fallire con un errore chiaro (`Response.status = 501 Not Implemented`) per gli altri schemi.**

---

### 3. Implementazione Server (`Server`)

#### A. Gestione Messaggio Entrante

* **`IntentType`:** Aggiungere **`IntentType::RequestUri`** per gestire la richiesta one-shot del client.
* **Validazione:** Dopo aver ricevuto `IntentType::RequestUri`, il Server deve validare i parametri: `client_version`, `username`/`password_hash`, `game_id`, e `uri`.
  * Per ora la validazione delle credenziali `username`/`password_hash` è sempre TRUE (come fatto anche negli altri casi). Se non è presente, centralizzare la funcione di controllo credenziali con una funzione che accetti `username`/`password_hash`, `game_id`, `client_version` e che per il moemnto torni sempre TRUE.

#### B. Flusso Event System

1.  **Invocazione Event System:** Il server deve invocare il Dispatcher dell'Event System con l'evento **`SystemEvents::RequestUri`**.
2.  **Preparazione Request/Response:** Il Core Server prepara gli oggetti `Request` e `Response` (incluso il pre-allocato `Response.buffer`), popolando `Request.Uri` con l'URI ricevuto dal client.
3.  **Dispatch:** Invocare la catena di handler (Mod registrati) per l'URI.
4.  **Risposta dell'Event System:** Al termine della catena (interruzione per `handled = true` o fine naturale), l'Event System restituisce l'oggetto `Response` finale.

#### C. Preparazione Risposta e Invio al Client

Il Server analizza l'oggetto `Response` e prepara il messaggio finale per il Client:

1.  **Lettura File (se necessario):** Se `Response.filepath` è valorizzato (non vuoto), il Server deve:
    * **Costruire il Path Assoluto:** Unire la root attuale dei dati con `Response.filepath`.
    * **Caricare il Contenuto:** Leggere l'intero contenuto del file in un array di byte (`file_content`).
    * **Estrarre il Nome:** Estrarre solo il nome del file dal `Response.filepath`.
2.  **Invio Risposta:** Inviare al Client i seguenti parametri serializzati, utilizzando il formato di risposta *PrimalMessage*:
    * `status` (`u16`, da `Response.status`).
    * `buffer` (Array di `u8`, la parte valida del `Response.buffer` se `Response.buffer_size > 0`).
    * `file_name` (Stringa, se il file è stato caricato).
    * `file_size` in bytes (ulong)
    * `file_content` (Array di byte in chunk). La grandezza del chunk deve essere dichiarata nel file di configurazione del server e come fallback è pari a 500kb (request_uri_chunk_size).
3.  **Gestione Server Error:** Se la lettura del file fallisce sul Server (es. file non trovato/permessi), il Server deve:
    * Impostare lo `status` a `500` o `404`.
    * Inviare questa risposta d'errore al Client.




# Network Download
ora implementiamo la api network.download che ho inserito nello script manager.js alla riga 58.
L'oggetto "network" e il metodo "download" deve esistere sia per il client che per il server quindi centralizza quanto più possibile.


## Client
Il metodo "download" deve:
- Analizzare l'uri e carpirne il protocollo.
- Se il protocollo è stam:// allora vuol dire che si il download deve essere fatto attraverso una connessione PrimalClient ad un server. Per fare ciò il client deve:
  - Utilizzare l'uri per evincere ip, porta user e password (eventuali).
  - Prelevare la route dall'url (ovvero cosa c'è dopo il primo slash dopo "ip:porta")
  - Connettersi al Server con una nuova socket 
  - Attendere il welcome message
  - Inviare PrimalMessage::Intent e IntentType::RequestUri con i seguenti argomenti:
    - client_version
    - username (se presente nell'uri)
    - password_hash (sha512) (se presente nell'uri)
    - game_id stringa (il game attuale su cui è configurato il client e che ha ricevuto dal server durante il "PrimalLogin")
    - uri della richiesta epurato della parte sulle credenziali.
  - Attendere i parametri di ritorno che sono:
    - status (ushort)
    - buffer se presente (array di u8)
    - se Response.filepath è valorizzato
      - file_name = il nome del file
      - file_content = l'intero contenuto del file (array di byte) 
  - Chiudere la socket per questa chiamata.

## Server
- Nel IntentType aggiungere "RequestUri" che sta ad indicare che un client sta inviando una richiesta oneshot che potrà essere interpretata anche da un mod (che si sarà registrato tramite l'EventSystem).
- I parametri richiesti sono:
  - client_version
  - username
  - password_hash (sha512)
  - game_id stringa
  - uri della richiesta, compresa la querystring (es: "stam://user:pass@127.0.0.1:9999/mods-manager/download/mod1?arg=value%arg2=value" oppure http://127.0.0.1:9999/mods-manager/download/mod1?arg=value%arg2=value oppure https://...)
- Il server invoca il DispachEvent(RequestUri) dell'eventsystem che chiama tutti gli handler per questo Uri.
- Al termine della catena di chiamate agli handler l'eventSystem restituisce la risposta e il server la manda al client nel seguente formato:
  - status = Response.status (ushort)
  - buffer = Response.buffer se presente (array di u8)
  - se Response.filepath è valorizzato
    - file_name = solo il nome del file in Response.filepath
    - file_content = l'intero contenuto del file (array di byte)


