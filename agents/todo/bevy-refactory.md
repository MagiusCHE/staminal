# Client Graphinc Multi engine.

Il client deve partire usando il thread principale solo per la gestione normale di stam_client, il 
Il thread principale deve gestire tutto in autonomia senza considerare bevy (come era prima di integrare bevy)

Quando poi uno script chiama enable_graphic_engine(GraphicEngines.Bevy) allora lo stam_client deve creare un nuovo thread e far partire l'app di bevy li dentro con un suo loop dediccato.
Quando il loop di bevy termina deve essere inviato un evento usando l'event system standard a tutti i mod registrati chiamato GraphicEngineEnded e come paramentro GraphicEngines.Bevy.

In questo modo tutti i mod (e magari il mod che ha avviato il graphic engine) sa che è terminato il motore grafico e può decidere se terminare o meno l'intero client.

Il loop principale attualemnte esce quando dalla console si preme CTRL+C. Introducendo i GraphicEngines, quando il loop principale termina, anche il loop del graphic engine deve terminare.

