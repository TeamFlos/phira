
multiplayer = Nhiều người chơi

connect = Kết nối
connect-must-login = Bạn phải đăng nhập để chơi nhiều người
connect-success = Kết nối thành công
connect-failed = Không thể kết nối
connect-authenticate-failed = Ủy quyển thất bại
reconnect = Kết nối lại...

create-room = Tạo phòng
create-room-success = Đã tạo!
create-room-failed = Không thể tạo phòng
create-invalid-id = ID phòng tối đa 20 ký tự, chứa chữ, số , - và _

join-room = Vào phòng
join-room-invalid-id = Không tìm thấy ID
join-room-failed = Không thể vào phòng

leave-room = Rời phòng
leave-room-failed = Không thể rời phòng

disconnect = Ngắt kết nối

request-start = Chơi
request-start-no-chart = Bạn phải chọn Chart
request-start-failed = Không thể bắt đầu

user-list = Người chơi

lock-room = { $current ->
  [true] Mở phòng
  *[other] Khóa phòng
}
cycle-room = { $current ->
  [true] Vòng lặp
  *[other] Bình thường
}

ready = Sẵn sàng
ready-failed = Không thể sẵn sàng

cancel-ready = Hủy

room-id = ID phòng: { $id }

download-failed = Không thể tải chart

lock-room-failed = Không thể khóa phòng
cycle-room-failed = Không thể đổi chế độ

chat-placeholder = Tin nhắn...
chat-send = Gửi
chat-empty = Không có văn bản
chat-sent = Đã gửi
chat-send-failed = Không thể gửi

select-chart-host-only = Chỉ có chủ phòng mới được chọn chart
select-chart-local = Không thể chọn chart của bạn
select-chart-failed = Không thể chọn chart
select-chart-not-now = Bạn không thể chọn lúc này.

msg-create-room = `{ $user }` đã tạo phòng
msg-join-room = `{ $user }` đã vào phòng
msg-leave-room = `{ $user }` đã rời
msg-new-host = `{ $user }` trở thành chủ phòng
msg-select-chart = `{ $user }` đã chọn `{ $chart }` (ID: #{ $id })
msg-game-start = `{ $user }` đã bắt đầu, đợi bạn sẵn sàng
msg-ready = `{ $user }` đã sẵn sàng
msg-cancel-ready = `{ $user }` hủy sẵn sàng
msg-cancel-game = `{ $user }` hủy trò chơi
msg-start-playing = Bắt đầu
msg-played = `{ $user }` đã kết thúc: { $score } ({ $accuracy }){ $full-combo ->
  [true] , FC
  *[other] {""}
}
msg-game-end = Kết thúc
msg-abort = Trò chơi bị hủy bởi `{ $user }`
msg-room-lock = { $lock ->
  [true] Đã khóa phòng
  *[other] Đã mở khóa phòng
}
msg-room-cycle = { $cycle ->
  [true] Chuyển sang chế độ Vòng lặp
  *[other] Chuyển về chế đọ Bình thường
}
