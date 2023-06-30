
multiplayer = Multiplayer

connect = Connect
connect-must-login = You must login to enter multiplayer mode
connect-success = Connected successfully
connect-failed = Failed to connect
connect-authenticate-failed = Authorization failed
reconnect = Reconnecting…

create-room = Create Room
create-room-success = Room created
create-room-failed = Failed to create room
create-invalid-id = Room ID consists of no more than 20 characters, including letters, numbers, - (dash) and _ (underscore)

join-room = Join Room
join-room-invalid-id = Invalid room ID
join-room-failed = Failed to join room

leave-room = Leave Room
leave-room-failed = Failed to leave room

disconnect = Disconnect

request-start = Start Game
request-start-no-chart = You haven't selected a chart
request-start-failed = Failed to start game

user-list = Users

lock-room = { $current ->
  [true] Unlock room
  *[other] Lock room
}
cycle-room = { $current ->
  [true] Cycling mode
  *[other] Normal mode
}

ready = Ready
ready-failed = Failed to get ready

cancel-ready = Cancel

room-id = Room ID: { $id }

download-failed = Failed to download chart

lock-room-failed = Failed to lock room
cycle-room-failed = Failed to change room mode

chat-placeholder = Say something…
chat-send = Send
chat-empty = Message is empty
chat-sent = Sent
chat-send-failed = Failed to send message

select-chart-host-only = Only the host can select chart
select-chart-local = Can't select local chart
select-chart-failed = Failed to select chart
select-chart-not-now = You can't select chart now

msg-create-room = `{ $user }` created the room
msg-join-room = `{ $user }` joined the room
msg-leave-room = `{ $user }` left the room
msg-new-host = `{ $user }` became the new host
msg-select-chart = The host `{ $user }` selected chart `{ $chart }` (#{ $id })
msg-game-start = The host `{ $user }` started the game. Other players should get ready.
msg-ready = `{ $user }` is ready
msg-cancel-ready = `{ $user }` cancelled ready
msg-cancel-game = `{ $user }` cancelled the game
msg-start-playing = Game start
msg-played = `{ $user }` finished playing: { $score } ({ $accuracy }){ $full-combo ->
  [true] , full combo
  *[other] {""}
}
msg-game-end = Game ended
msg-abort = `{ $user }` aborted the game
msg-room-lock = { $lock ->
  [true] Room locked
  *[other] Room unlocked
}
msg-room-cycle = { $cycle ->
  [true] Room changed to cycling mode
  *[other] Room changed to normal mode
}
