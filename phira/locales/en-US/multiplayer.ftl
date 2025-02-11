
multiplayer = Multiplayer

connect = Connect
connect-must-login = You must login to access multiplayer functionality.
connect-success = Connected successfully.
connect-failed = Failed to connect.
connect-authenticate-failed = Authorization failed.
reconnect = Reconnecting…

create-room = Create Room
create-room-success = Room created.
create-room-failed = Failed to create room.
create-invalid-id = A Room ID should be 20 characters max, and only contain A-Z, a-z, 0-9, -, and _.

join-room = Join Room
join-room-invalid-id = Invalid room ID.
join-room-failed = Failed to join room.

leave-room = Leave Room
leave-room-failed = Failed to leave room.

disconnect = Disconnect

request-start = Start Game
request-start-no-chart = Select an online chart first.
request-start-failed = Failed to start.

user-list = Users

lock-room = { $current ->
  [true] Unlock Room
  *[other] Lock Room
}
cycle-room = { $current ->
  [true] Cycle Mode
  *[other] Normal Mode
}

ready = Ready
ready-failed = Failed to get ready.

cancel-ready = Cancel

room-id = Room ID: { $id }

download-failed = Failed to download chart.

lock-room-failed = Failed to lock room.
cycle-room-failed = Failed to change room mode.

chat-placeholder = Type a message...
chat-send = Send
chat-empty = Message is empty.
chat-sent = Sent
chat-send-failed = Failed to send message.

select-chart-host-only = Only the host can select charts.
select-chart-local = You can only select online charts.
select-chart-failed = Failed to select chart.
select-chart-not-now = You can't select chart now.

msg-create-room = `{ $user }` created the room.
msg-join-room = `{ $user }` joined the room.
msg-leave-room = `{ $user }` left the room.
msg-new-host = `{ $user }` became the new host.
msg-select-chart = The host `{ $user }` selected chart `{ $chart }` (#{ $id }).
msg-game-start = The host `{ $user }` started the game.
msg-ready = `{ $user }` is ready.
msg-cancel-ready = `{ $user }` canceled being ready.
msg-cancel-game = `{ $user }` cancelled the game.
msg-start-playing = Game started.
msg-played = `{ $user }` finished playing: { $score } ({ $accuracy }){ $full-combo ->
  [true] , full combo.
  *[other] {""}.
}
msg-game-end = Game ended.
msg-abort = `{ $user }` aborted the game.
msg-room-lock = { $lock ->
  [true] Room locked.
  *[other] Room unlocked.
}
msg-room-cycle = { $cycle ->
  [true] Room mode changed to Cycle.
  *[other] Room mode changed to Normal.
}
