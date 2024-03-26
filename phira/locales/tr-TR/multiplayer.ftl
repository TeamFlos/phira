
multiplayer = Çok Oyunculu

connect = Bağlan
connect-must-login = Çok oyunculu moda katılmak için giriş yapmalısınız
connect-success = Bağlantı başarılı
connect-failed = Bağlanılamadı
connect-authenticate-failed = Giriş başarısız
reconnect = Yeniden bağlanılıyor...

create-room = Oda Yarat
create-room-success = Oda yaratıldı
create-room-failed = Oda yaratılamadı
create-invalid-id = Oda ID'si, 20 karakterden uzun olmayacak şekilde harf, sayı, kısa çizgi (-) ve alt çizgi (_) içerebilir

join-room = Bir Odaya Katıl
join-room-invalid-id = Geçersiz Oda ID'si
join-room-failed = Odaya katılınamadı

leave-room = Odadan Ayrıl
leave-room-failed = Odadan ayrılınamadı

disconnect = Bağlantıyı Kes

request-start = Oyuna Başla
request-start-no-chart = Bir müzik seçmediniz
request-start-failed = Oyun başlatılamadı

user-list = Kullanıcılar

lock-room = { $current ->
  [true] Odanın kilidini aç
  *[other] Odayı kilitle
}
cycle-room = { $current ->
  [true] Dönen mod
  *[other] Normal mod
}

ready = Hazır
ready-failed = Hazırlanma başarısız

cancel-ready = İptal

room-id = Oda ID'si: { $id }

download-failed = Müzik indirilemedi

lock-room-failed = Oda kilitlenemedi
cycle-room-failed = Oda modu değiştirilemedi

chat-placeholder = Bir şey yazın...
chat-send = Gönder
chat-empty = Yaprak bile kıpırdamıyor
chat-sent = Gönderildi
chat-send-failed = Mesaj gönderilemedi

select-chart-host-only = Müziği yalnızca oda sahibi seçebilir
select-chart-local = Yerel müzik seçilemiyor
select-chart-failed = Müzik seçilemedi
select-chart-not-now = Şu anda müzik seçemezsiniz

msg-create-room = `{ $user }` odayı yarattı
msg-join-room = `{ $user }` odaya katıldı
msg-leave-room = `{ $user }` odadan ayrıldı
msg-new-host = `{ $user }` yeni oda sahibi 
msg-select-chart = Oda sahibi `{ $user }`, `{ $chart }` (#{ $id }) müziğini seçti
msg-game-start = Oda sahibi `{ $user }` oyunu başlatıyor. Oyuncular hazırlansın.
msg-ready = `{ $user }` hazır
msg-cancel-ready = `{ $user }` artık hazır değil
msg-cancel-game = `{ $user }` oyunu iptal etti
msg-start-playing = Oyun başladı
msg-played = `{ $user }` bitirdi: { $score } ({ $accuracy }){ $full-combo ->
  [true] , full kombo
  *[other] {""}
}
msg-game-end = Oyun bitti
msg-abort = `{ $user }` oyunu iptal etti
msg-room-lock = { $lock ->
  [true] Oda kilitlendi
  *[other] Odanın kilidi açıldı
}
msg-room-cycle = { $cycle ->
  [true] Oda dönen moda geçirildi
  *[other] Oda normal moda geçirildi
}
