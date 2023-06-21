
multiplayer = Multiplayer

connect = Connect
connect-must-login = You must login to enter multiplayer mode
connect-success = Connected successfully
connect-failed = Failed to connect
connect-authorize-failed = Authorization failed

create-room = Create Room
create-room-success = Room created
create-room-failed = Failed to create room

join-room = Join Room
join-room-invalid-id = Invalid room ID
join-room-failed = Failed to join room

leave-room = Leave Room
leave-room-failed = Failed to leave room

disconnect = Disconnect

request-start = Start Game
request-start-no-chart = You haven't selected a chart
request-start-failed = Failed to start game

ready = Ready
ready-failed = Failed to get ready

cancel-ready = Cancel

room-id = Room ID: { $id }

download-failed = Failed to download chart

chat-placeholder = Say something…
chat-send = Send
chat-empty = Message is empty
chat-sent = Sent
chat-send-failaed = Failed to send message

select-chart-host-only = Only the host can select chart
select-chart-local = Can't select local chart
select-chart-failed = Failed to select chart

msg-create-room = `{ $user }` created the room
msg-join-room = `{ $user }` joined the room
msg-leave-room = `{ $user }` left the room
msg-new-host = `{ $user }` became the new host
msg-select-chart = The host `{ $user }` selected chart `{ $chart }` (#{ $id })
msg-game-start = The host `{ $user }` started the game
msg-ready = `{ $user }` is ready
msg-cancel-ready = `{ $user }` cancelled ready
msg-cancel-game = `{ $user }` cancelled the game
msg-start-playing = Game start
msg-played = `{ $user }` finished playing: { $score } ({ $accuracy }){ $full-combo ->
  [true] , full combo
  *[other] {""}
}
msg-game-end = Game ended
