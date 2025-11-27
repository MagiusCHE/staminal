ora implementiamo la api network.download che ho inserito nello script manager.js alla riga 58.
L'oggetto network e il metodo download deve esistere sia per il client che per il server quindi centralizza quanto pià possibile.

Il metodo download deve:

- Analizzare l'url e carpirne il protocollo.
- Se il protocollo è stam:// allora vuol dire che si il download deve essere fatto attraverso una connessione PrimalClient ad un server. Per fare ciò il client deve:
  - Utilizzare l'url per evincere ip, porra user e password (eventuali).
  - Prelevare l'intendo dall'url (ovvero cosa c'è dopo il primo slash dopo "ip:porta")
  - Connettersi al Server e dichiarare l'itento OneShotRequest
  - Attendere il welcome message.
  - Inviare la query string dell'url.
  - Attendere il file (che deve essere sempre uno sip)



# Server
- Nel IntentType aggiungere "UriRequest" che sta ad indicare che un client sta inviando una richiesta oneshot che potrà essere interpretata anche da un mod (che si sarà registrato).
- I parametri richiesti sono:
  - user
  - pass
  - client version
  - uri della richiesta, compresa la querystring (es: "stam://user:pass@127.0.0.1:9999/mods-manager/download/mod1?arg=value%arg2=value" oppure http://127.0.0.1:9999/mods-manager/download/mod1?arg=value%arg2=value oppure https://...)
- Il server controlla in una lista di registrazione se un mod si è registrato per risolvere questa route.
- Nel runtime JS I mod si possono registrare durante l'onAttach usando il metodo syncrono system.register_event(SystemEvents.UriRequest,stam://mods-manager/download", this.handle_mod_download_request.bind(this));
  - La funzione system.register_event ha come argomenti:
    - arg1: per il momento i soli parametri accettati sono 
- Per ogni mod registrato il server invoca l'handler che il mod ha indicato (in base la runtime) passndo l'oggetto "request" e "response". L'handler viene invocato in modo asyncrono e il server attende con await che tutti gli handler registrati abbiano completato.

# Server e Client
- Creare un sistema di registrazione eventi per cui ogni mod durante l'"onAttach" può registrarsi a determinati eventi che il core potrà scatenare.
- Quando richiesto il runtime dovrà dispatchare questi eventi in modo asyncrono ma sequenziali rispettando l'ordine di priorità indicata durante la registrazione dell'evento.
- Il dispatch dell'evento prevede che venga passato un oggetto "Request" che contiene le informazioni specifiche di ogni evento e un oggetto "Response" che l'handler potrà usare. L'oggetto Response dovrà poi essere passato al successivo handler (se presente) a meno ché la proprietà Response.handled sia "true". Se al termine della catena di chiamate degli handler la proprietà è ancora "false" o "undefined" allora deve essere lanciato un errore non critico sul server (il server non deve crashare) ma l'errore deve essere loggato. Dopo ogni chiamata 
  - L'oggetto request deve contenere i dati ricevuti in chiamata e la modifica di questo non si deve riperquotere nella catena.
  - L'oggetto response deve contenere una proprietà "context" che deve essere un hashtable che il mod può rimepire a piacimento per passare/memorizare parametri che poi potranno essere letti dai successivi handler.
  - Ogni evento specifico attenderà dei dati specifici immagazzinati nella Response per i suoi scopi.
  - Al termine della catena l'oggetto Requestre Response vengono distrutti liberando le risorse.
- per registrarsi ad un evento il runtime javascript (ma questa cosa va ovviamente pensata per essere estesa a tutti gli altri scripting language previsti) deve usare la funzione register_event dell'oggetto "system" con signature (SystemEvents, handler, variadic args);
  - SystemEvents è un enum che al momento ha un solo valore "UriRequest". In javascript si può creare l'enum durante la registrazione dell'api system che mappi quello interno di rust
  - handler è il puntamento alla funzione che deve essere chiamata dal core quando si scatena l'evento
  - args sono parametri vari opzionali (può essere più di uno) che l'evento richiede
- Al momento l'unico evento disponibile è SystemEvents.UriRequest
  - Quando la catena di mod riceverà questo evento esso avrà:
    - Request.Uri = un uri richiesto per esempio stam://12.7.0.0.1:9999/mods-manager/download/mio_mod
    - Response.buffer = un'array preallocato da rust (&mut [u8]) in cui il mod può scrivere.
    - Response.filepath = una string che rappresenta il path relativo di un file. La root del path è la 
    - Nota: ogni chiamata buffer_clear o _write deve allocare o scrivere memoria nel buffer che rust gestirà dinamicamente. Questo buffer verrà poi copiato 
  - Il mod potrà riempire il response.buffer