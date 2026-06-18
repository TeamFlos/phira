
multiplayer = Multiplayer

connect = Hubungkan
connect-must-login = Anda harus login untuk masuk ke mode multiplayer
connect-success = Berhasil terhubung
connect-failed = Gagal terhubung
connect-authenticate-failed = Otorisasi gagal
reconnect = Menghubungkan...

create-room = Buat room
create-room-success = Room dibuat
create-room-failed = Gagal membuat room
create-invalid-id = ID room terdiri dari tidak lebih dari 20 karakter, termasuk huruf, angka, - (tanda hubung) dan _ (garis bawah)

join-room = Bergabung ke Room
join-room-invalid-id = ID room tidak valid
join-room-failed = Gagal bergabung ke dalam room

leave-room = Meninggalkan Room
leave-room-failed = Gagal meninggalkan room

disconnect = Memutuskan

request-start = Mulai permainan
request-start-no-chart = Anda belum memilih chart
request-start-failed = Gagal memulai permainan

user-list = Pemain

lock-room = { $current ->
  [true] Room terbuka
  *[other] Room terkunci
}
cycle-room = { $current ->
  [true] Mode cycling
  *[other] Mode normal
}

ready = Siap
ready-failed = Gagal bersiap

cancel-ready = Batal

room-id = Room ID: { $id }

download-failed = Gagal mengunduh chart

lock-room-failed = Gagal mengunci room
cycle-room-failed = Gagal mengubah mode room

chat-placeholder = Katakan sesuatu...
chat-send = Kirim
chat-empty = Pesan kosong
chat-sent = Terkirim
chat-send-failed = Gagal mengirim pesan

select-chart-host-only = Hanya host yang dapat memilih chart
select-chart-local = Tidak dapat memilih chart lokal
select-chart-failed = Gagal untuk memilih chart
select-chart-not-now = Anda tidak dapat memilih chart sekarang

msg-create-room = `{ $user }` membuat room
msg-join-room = `{ $user }` bergabung ke dalam room
msg-leave-room = `{ $user }` keluar dari room
msg-new-host = `{ $user }` menjadi host baru
msg-select-chart = Host `{ $user }`memilih chart `{ $chart }` (#{ $id })
msg-game-start = Host `{ $user }` memulai permainan. Pemain lain harus segera bersiap.
msg-ready = `{ $user }` siap
msg-cancel-ready = `{ $user }` membatalkan siap
msg-cancel-game = `{ $user }` membatalkan permainan
msg-start-playing = Permainan dimulai
msg-played = `{ $user }` selesai bermain: { $score } ({ $accuracy }){ $full-combo ->
  [true] , full combo
  *[other] {""}
}
msg-game-end = Permainan berakhir
msg-abort = `{ $user }` membatalkan permainan
msg-room-lock = { $lock ->
  [true] Room terkunci
  *[other] Room terbuka
}
msg-room-cycle = { $cycle ->
  [true] Room berubah menjadi mode cycling
  *[other] Room berubah menjadi mode normal
}
