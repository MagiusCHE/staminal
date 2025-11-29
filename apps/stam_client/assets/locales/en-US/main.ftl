# Staminal Client - English Locale

## Connection messages
connecting = Connecting to {$host}...
connected = Connected to {$host}
connection-failed = Failed to connect: {$error}
connection-closed = Connection closed by server
disconnecting = Disconnecting...

## Authentication
login-sending = Sending login credentials...
login-success = Successfully logged in!
login-failed = Login failed: {$reason}
version-check = Checking version compatibility...
version-compatible = Version compatible: {$client} ~ {$server}
version-mismatch = Version mismatch! Client: {$client}, Server: {$server}

## Server messages
server-welcome = Received welcome from server, version: {$version}
server-list-received = Received server list with {$count} server(s)
server-list-empty = Server list is empty, no game servers available
server-error = Server error: {$message}

## Game client
game-connecting = Connecting to game server at {$host}
game-connected = Connected to game server
game-login-success = Successfully logged into game server!
game-client-ready = Game client connected, waiting for messages (Ctrl+C to disconnect)...
game-client-ready-no-hint = Game client connected, waiting for messages...
game-shutdown = Shutting down game client...

## Disconnect reasons (these IDs are sent by server)
disconnect-server-shutdown = Server is shutting down
disconnect-kicked = You have been kicked from the server
disconnect-banned = You have been banned from the server
disconnect-idle-timeout = Disconnected due to inactivity
disconnect-version-mismatch = Client version incompatible with server
disconnect-maintenance = Server is under maintenance
disconnect-unknown = Disconnected from server

## Errors
error-invalid-uri = Invalid URI scheme: {$uri}
error-unexpected-message = Unexpected message received
error-parse-failed = Failed to parse server response
js-fatal-error = Fatal JavaScript error in mod, client shutting down

## General
ctrl-c-received = Ctrl+C received, disconnecting...
shutting-down = Shutting down client...
