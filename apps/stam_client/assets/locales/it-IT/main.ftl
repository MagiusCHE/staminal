# Staminal Client - Locale Italiana

## Messaggi di connessione
connecting = Connessione a {$host}...
connected = Connesso a {$host}
connection-failed = Connessione fallita: {$error}
connection-closed = Connessione chiusa dal server
disconnecting = Disconnessione in corso...

## Autenticazione
login-sending = Invio credenziali di accesso...
login-success = Accesso effettuato con successo!
login-failed = Accesso fallito: {$reason}
version-check = Verifica compatibilità versione...
version-compatible = Versione compatibile: {$client} ~ {$server}
version-mismatch = Versione incompatibile! Client: {$client}, Server: {$server}

## Messaggi del server
server-welcome = Ricevuto benvenuto dal server, versione: {$version}
server-list-received = Ricevuta lista server con {$count} server
server-list-empty = Lista server vuota, nessun game server disponibile
server-error = Errore del server: {$message}

## Client di gioco
game-connecting = Connessione al game server a {$host}
game-connected = Connesso al game server
game-login-success = Accesso al game server effettuato con successo!
game-client-ready = Client di gioco connesso, in attesa di messaggi (Ctrl+C per disconnettersi)...
game-shutdown = Arresto del client di gioco...

## Motivi di disconnessione (questi ID sono inviati dal server)
disconnect-server-shutdown = Il server si sta spegnendo
disconnect-kicked = Sei stato espulso dal server
disconnect-banned = Sei stato bannato dal server
disconnect-idle-timeout = Disconnesso per inattività
disconnect-version-mismatch = Versione del client incompatibile con il server
disconnect-maintenance = Il server è in manutenzione
disconnect-unknown = Disconnesso dal server

## Errori
error-invalid-uri = Schema URI non valido: {$uri}
error-unexpected-message = Ricevuto messaggio inaspettato
error-parse-failed = Impossibile interpretare la risposta del server
js-fatal-error = Errore JavaScript fatale nel mod, arresto del client

## Generale
ctrl-c-received = Ricevuto Ctrl+C, disconnessione in corso...
shutting-down = Arresto del client...
