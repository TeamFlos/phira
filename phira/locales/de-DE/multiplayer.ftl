
multiplayer = Mehrspieler

connect = Verbinden
connect-must-login = Du musst eingeloggt sein, um den Mehrspieler-Modus zu nutzen.
connect-success = Verbindung erfolgreich.
connect-failed = Verbindung fehlgeschlagen.
connect-authenticate-failed = Autorisierung fehlgeschlagen.
reconnect = Verbindung wird wiederhergestellt…

create-room = Raum erstellen
create-room-success = Raum erstellt.
create-room-failed = Raum konnte nicht erstellt werden.
create-invalid-id = Eine Raum-ID darf maximal 20 Zeichen enthalten und nur A-Z, a-z, 0-9, -, _ nutzen.

join-room = Raum beitreten
join-room-invalid-id = Ungültige Raum-ID.
join-room-failed = Beitritt zum Raum fehlgeschlagen.

leave-room = Raum verlassen
leave-room-failed = Raum konnte nicht verlassen werden.

disconnect = Trennen

request-start = Spiel starten
request-start-no-chart = Wähle zuerst ein Online-Level aus.
request-start-failed = Spiel konnte nicht gestartet werden.

user-list = Benutzerliste

lock-room = { $current ->
  [true] Raum entsperren
  *[other] Raum sperren
}
cycle-room = { $current ->
  [true] Round-Robin Modus
  *[other] Normalmodus
}

ready = Bereit
ready-failed = Bereitmeldung fehlgeschlagen.

cancel-ready = Abbrechen

room-id = Raum-ID: { $id }

download-failed = Level konnte nicht heruntergeladen werden.

lock-room-failed = Raum konnte nicht gesperrt werden.
cycle-room-failed = Raummodus konnte nicht geändert werden.

chat-placeholder = Nachricht eingeben…
chat-send = Senden
chat-empty = Nachricht ist leer.
chat-sent = Gesendet
chat-send-failed = Nachricht konnte nicht gesendet werden.

select-chart-host-only = Nur der Host kann Levels auswählen.
select-chart-local = Du kannst nur Online-Levels auswählen.
select-chart-failed = Level konnte nicht ausgewählt werden.
select-chart-not-now = Du kannst gerade kein Level auswählen.

msg-create-room = `{ $user }` hat den Raum erstellt.
msg-join-room = `{ $user }` ist dem Raum beigetreten.
msg-leave-room = `{ $user }` hat den Raum verlassen.
msg-new-host = `{ $user }` ist der neue Host.
msg-select-chart = Der Host `{ $user }` hat das Level `{ $chart }` (#{ $id }) ausgewählt.
msg-game-start = Der Host `{ $user }` hat das Spiel gestartet..
msg-ready = `{ $user }` ist bereit.
msg-cancel-ready = `{ $user }` ist nicht mehr bereit.
msg-cancel-game = `{ $user }` hat das Spiel abgebrochen.
msg-start-playing = Spiel gestartet.
msg-played = `{ $user }` hat Level abgeschlossen: { $score } ({ $accuracy }){ $full-combo ->
  [true] , Full Combo.
  *[other] {""}.
}
msg-game-end = Spiel beendet.
msg-abort = `{ $user }` hat das Spiel abgebrochen.
msg-room-lock = { $lock ->
  [true] Raum gesperrt.
  *[other] Raum entsperrt.
}
msg-room-cycle = { $cycle ->
  [true] Raummodus geändert zu Round-Robin.
  *[other] Raummodus geändert zu Normal.
}
