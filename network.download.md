ora implementiamo la api network.download che ho inserito nello script manager.js alla riga 58.
L'oggetto network e il metodo download deve esistere sia per il client che per il server quindi centralizza quanto pià possibile.

Il metodo download deve:

- Analizzare l'url e carpirne il protocollo.
- Se il protocollo è stam:// allora vuol dire che si il download deve essere fatto attraverso una connessione PrimalClient ad un server. Per fare ciò il client deve:
  - Utilizzare l'url per evincere ip, porra user e password (eventuali).
  - Prelevare l'intendo dall'url (ovvero cosa c'è dopo il primo slash dopo "ip:porta")
  - Connettersi al Server e dichiarare l'itento ModRequest (aggiungere IntentType.ModRequest)
  - Attendere il welcome message.
  - Inviare la query string dell'url.
  - Attendere il file (che deve essere sempre uno sip)
