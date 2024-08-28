
multiplayer = Олон тоглогч

connect = Холбогдох
connect-must-login = Олон тоглогчийн горимд орохын тулд та нэвтэрсэн байх ёстой
connect-success = Амжилттай холбогдсон
connect-failed = Холбогдоход амжилтгүй боллоо
connect-authenticate-failed = Зөвшөөрөл олгогдоход амжилтгүй боллоо
reconnect = Дахин холбогдож байна...

create-room = Өрөө үүсгэх
create-room-success = Өрөө үүсгэгдсэн
create-room-failed = Өрөө үүсгэхэд амжилтгүй болов
create-invalid-id = Өрөөний ID нь үсэг, тоо, - (зураас), _ (доогуур зураас) зэргийг багтаасан 20-с илүүгүй тэмдэгтээс бүрдэнэ

join-room = Өрөө рүү орох
join-room-invalid-id = Буруу өрөөний ID
join-room-failed = Өрөө рүү ороход амжилтгүй боллоо

leave-room = Өрөөнөөс гарах
leave-room-failed = Өрөөнөөс гарахад амжилтгүй боллоо

disconnect = Таслах

request-start = Тоглоом Эхлүүлэх
request-start-no-chart = Та чарт сонгоогүй байна
request-start-failed = Тоглоом эхлүүлэхэд амжилтгүй боллоо

user-list = Хэрэглэгчид

lock-room = { $current ->
  [true] Өрөөг онгойлгох
  *[other] Өрөөг цоожлох
}
cycle-room = { $current ->
  [true] Дугуйн горим
  *[other] Энгийн горим
}

ready = Бэлэн болох
ready-failed = Бэлэн болоход амжилтгүй боллоо

cancel-ready = Цуцлах

room-id = Өрөөний ID: { $id }

download-failed = Чартыг татахад амжилтгүй боллоо

lock-room-failed = Өрөөг цоожлоход амжилтгүй боллоо
cycle-room-failed = Өрөөний горимыг өөрчлөхөд амжилтгүй боллоо

chat-placeholder = Ямар нэг юм хэлээрэй...
chat-send = Илгээх
chat-empty = Мессеж хоосон байна
chat-sent = Илгээгдсэн
chat-send-failed = Мессеж илгээхэд амжилтгүй боллоо

select-chart-host-only = Зөвхөн өрөөний эзэн чартуудыг сонгоно
select-chart-local = Can't select local chart
select-chart-failed = Чартыг сонгоход амжилтгүй боллоо
select-chart-not-now = Та яг одоо чартыг сонгох боломжгүй

msg-create-room = `{ $user }` өрөөг үүсгэв
msg-join-room = `{ $user }` өрөө рүү орлоо
msg-leave-room = `{ $user }` өрөөнөөс гарлаа
msg-new-host = `{ $user }` шинэ өрөөний эзэн боллоо
msg-select-chart = Өрөөний эзэн `{ $user }` чарт `{ $chart }` (#{ $id }) сонголоо
msg-game-start = Өрөөний эзэн `{ $user }` тоглоомоо эхлүүлэв. Бусад тоглогчид бэлэн болоорой.
msg-ready = `{ $user }` бэлэн болов
msg-cancel-ready = `{ $user }` бэлэн болохоо болиулав
msg-cancel-game = `{ $user }` тоглоомоо болиуллаа
msg-start-playing = Game Start
msg-played = `{ $user }` тоглоод дууссан: { $score } ({ $accuracy }){ $full-combo ->
  [true] , full combo
  *[other] {""}
}
msg-game-end = Тоглоом дууслаа
msg-abort = `{ $user }` тоглоомоо зогсоолоо
msg-room-lock = { $lock ->
  [true] Өрөө цоожлогдсон
  *[other] Өрөө онгойлгогдсон
}
msg-room-cycle = { $cycle ->
  [true] Өрөө дугуйн горим руу өөрчлөгдөв
  *[other] Өрөө энгийн горим руу өөрчлөгдөв
}
