
multiplayer = Multiplayer

connect = เชื่อมต่อ
connect-must-login = คุณต้อง Login เพื่อเล่น Multiplayer
connect-success = เชื่อมต่อเสร็จสิ้น
connect-failed = เชื่อมต่อล้มเหลว
connect-authenticate-failed = การขออนุญาติล้มเหลว
reconnect = เชื่อมต่ออีกครั้ง…

create-room = สร้างห้อง
create-room-success = สร้างห้องเสร็จสิ้น
create-room-failed = ไม่สามารถสร้างห้องได้
create-invalid-id = ID ห้องไม่ควรเกิน 20 ตัว ต้องมีตัวอักษร หลังจากนี้จะไม่มีก็ได้ ตัวเลข, - (เครื่องขีด) และ _ (ขีดเส้นใต้)

join-room = เข้าร่วมห้อง
join-room-invalid-id = ID ไม่ถูกต้อง
join-room-failed = ไม่สามารถเข้าร่วมได้

leave-room = ออกห้อง
leave-room-failed = ไม่สามารถออกห้องได้

disconnect = ตัดการเชื่อมต่อ

request-start = เริ่มเกม
request-start-no-chart = คุณยังไม่ได้เลือก Chart
request-start-failed = ไม่สามารถเริ่มเกมได้

user-list = Users

lock-room = { $current ->
  [true] ปลดล็อคห้อง
  *[other] ล็อคห้อง
}
cycle-room = { $current ->
  [true] โหมด วน
  *[other] โหมด ธรรมดา
}

ready = พร้อม
ready-failed = ไม่สามารถพร้อมได้

cancel-ready = ยกเลิก

room-id = ID ห้อง: { $id }

download-failed = ไม่สามารถ Download chart ได้

lock-room-failed = ไม่สามารถล็อคห้องได้
cycle-room-failed = ไม่สามารถเปลี่ยนโหมดได้

chat-placeholder = ลองพูดอะไรสักอย่าง…
chat-send = ส่ง
chat-empty = ว่างเปล่า :v
chat-sent = ส่งเรียบร้อย
chat-send-failed = ไม่สามารถส่งข้อความได้

select-chart-host-only = มีแค่ Host เท่านั้นที่เลือก Chart ได้
select-chart-local = ไม่สามารถเลือก Chart จาก Local ได้
select-chart-failed = ไม่สามารถเลือก Chart ได้
select-chart-not-now = คุณไม่สามารถเลือก Chart ได้

msg-create-room = `{ $user }` ได้สร้างห้องแล้ว.
msg-join-room = `{ $user }` ได้เข้าห้องแล้ว.
msg-leave-room = `{ $user }` ได้ออกจากห้องแล้ว.
msg-new-host = `{ $user }` ได้เป็น Host คนใหม่แล้ว.
msg-select-chart = Host `{ $user }` ได้เลือก Chart `{ $chart }` (#{ $id })
msg-game-start = Host `{ $user }` ได้เริ่มเกมแล้ว. ผู้เล่นคนอื่คนจะพร้อมได้แล้ว.
msg-ready = `{ $user }` พร้อมแล้ว.
msg-cancel-ready = `{ $user }` ยกเลิกพร้อมแล้ว.
msg-cancel-game = `{ $user }` ยกเลิกเกมแล้ว.
msg-start-playing = เกมได้เริ่มแล้ว.
msg-played = `{ $user }` เล่นเสร็จแล้ว: { $score } ({ $accuracy }){ $full-combo ->
  [true] , full combo
  *[other] {""}
}
msg-game-end = เกมได้จบลงแล้ว.
msg-abort = `{ $user }` เกมได้ยกเลิกแล้ว.
msg-room-lock = { $lock ->
  [true] ห้องถูกล็อคแล้ว.
  *[other] ห้องถูกปลดล็อคแล้ว.
}
msg-room-cycle = { $cycle ->
  [true] ห้องถูกเปลี่ยนเป็นโหมดวนแล้ว.
  *[other] ห้องได้ถูกเปลี่ยนเป็นโหมดธรรมดาแล้ว.
}
