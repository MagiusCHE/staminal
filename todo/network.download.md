## Event System
- Creare un sistema di registrazione eventi per cui ogni mod durante l'"onAttach" può registrarsi a determinati eventi che il core potrà scatenare.
- Ogni mod può registrare infiniti handler a patto che puntino a funzioni diverse.
- Le registrazioni persistono fino alla chiusura dell'app.
- Quando richiesto il runtime dovrà dispatchare questi eventi in modo asyncrono ma sequenziale rispettando l'ordine di priorità indicata durante la registrazione dell'evento.
- L'evento per essere registrato richiede:
  - Il nome dell'evento (enum SystemEvents)
  - Il runtime che l'ha richiesto
  - Il context del runtime che l'ha richiesto
  - La funzionme handler dello script da invocare
  - La priorità (un numero intero con segno) che indica l'ordine con cui eseguire la catena di eventi (dal più piccolo al più grande)
  - Altri argomenti specifici di ogni evento.
- per registrarsi ad un evento il mod javascript (ma questa cosa va ovviamente pensata per essere estesa a tutti gli altri scripting language previsti) deve usare la funzione register_event dell'oggetto "system" con signature (SystemEvents, handler, priority, variadic args);
  - SystemEvents è l'enum del nome dell'evento. In javascript si può creare l'enum durante la registrazione dell'api system che mappi quello interno di rust e la stessa dcosa andrà fatta sugli altri linguaggi.
  - handler è il puntamento alla funzione che deve essere chiamata dal core quando si scatena l'evento
  - priority è un intero con segno che indica l'ordine con cui eseguire la catena di eventi (dal più piccolo al più grande)
  - args sono parametri vari (può essere più di uno) che l'evento richiede
- Al momento l'unico evento disponibile è SystemEvents.UriRequest
- 
### SystemEvents.UriRequest
Per ora prepariamo questo evento e quando sarà pronto implementeremo il punto in cui viene chiamato e gestiremo le risposte al chiamante.

Il flusso dovrà essere questo:
- Per registrarsi a questo evento lo script deve imporre come parametri vari obbligatori:
  - Protocollo (non obbligatorio, default .All): un enum ReuqestUriProtocols che ha 2 valori .Stam, .Http, .All
  - Route (non obbligatorio): una stringa che rappresenta la parte iniziale dell'Uri che l'handler vuole ricevere.
- Quando il core decide di scatenare questo evento prepara i 2 oggetti Request e Response che devono persistere per tutta la durata della catena. Gli oggetti saranno così composti:
  - Request.Uri = un uri richiesto per esempio stam://12.7.0.0.1:9999/mods-manager/download/mio_mod
    - Il core chiamerà solo gli handler che hanno imposto come Protocollo proprio quello che l'uri ha. Se l'evento è stato registrato con protocollo .All allora potrà ricevere la chaiamta indipendentemente dal protocollo dell'uri
    - Il core chiamerà solo gli handler che hanno imposto come Route la parte che si trova dopo il dns:porta (senza lo slash inziale) o se lo script si è registrato senza indicare la route.
  - Response.status = (default 404) un intero assimilabile allo standard http (500 per errore interno al aserver, 200 per tutto ok)
  - Response.buffer = un'array preallocato da rust (&mut [u8]) in cui il mod può scrivere (dimensione fissa e unico durante tutta l'esecuzione dell'evento per tutti i mod della catena)
    - Per la configurazione di questo evento va aggiunto un parametro nel file di configurazione all'interno di un game chiamato "event_buffer_size" (l'ho aggiunto già nel file apps/stam_server/workspace_data/configs/develop.json). Il buffer sarà preallocato con queste dimensioni.
  - Response.buffer_size (inizialmente a 0) che indica il numeor di bytes scritti dal mod e presenti nel buffer
  - Response.filepath (vuota inizialmente) = una string che rappresenta il path relativo di un file. La root del path è la root attuale dei dati (dove si trova la cartella mods per capirci)
- Quando la catena dei mod finisce il Response.buffer, Response.buffer_size, Response.flepath, Response.status saranno a disposizione del core per essere inviati a chi ne ha fatto richiesta.
- Se viene generato un'eccezione dutante la chiamata ad uno quealsiasi dei mod della catena il Response.status deve essere impostato a 500, il buffer_size deve essere impostato a 0 e il filepath deve essere svuotato e si deve passare al successivo scipt handler della catena.
- Sia in caso di errore che in caso di successo alla fine dell'evento le seguenti variabili devono essere inviate a chi ha scatenato l'evento:
  - lo status
  - la parte piena del buffer
  - il filepath

Come già detto al momento non c'è nessuno che scatena questo evento ma è impostante preparare la struttura e testare la registrazione.

#### Test
Nel mod Mods-Manager ho già inserito la registrazione dell'evento nell'onAttach.
Aggiunti un test per provare questo evento RequestUri (il test verrà poi esteso per altri casi)





----------------------------------------------

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

# Server e Client Event system

Parti creando una libreria shared che gestisce il sistema di eventi. La libreria potrà essere usata sia dal client che dal server.


