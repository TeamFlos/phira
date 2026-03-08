
multiplayer = Nhiều người chơi

connect = Kết nối
connect-must-login = Bạn phải đăng nhập để sử dụng tính năng này
connect-success = Đã kết nối
connect-failed = Không thể kết nối
connect-authenticate-failed = Uỷ quyền thất bại
reconnect = Đang kết nối lại...

create-room = Tạo phòng
create-room-success = Đã tạo phòng
create-room-failed = Không thể tạo phòng
create-invalid-id = ID phòng phải dưới 20 ký tự, có thể chứa chữ, số, - và _

join-room = Vào phòng
join-room-invalid-id = Không tìm thấy ID phòng
join-room-failed = Không thể vào phòng

leave-room = Rời phòng
leave-room-failed = Không thể rời phòng

disconnect = Ngắt kết nối

request-start = Bắt đầu
request-start-no-chart = Bạn chưa chọn Chart
request-start-failed = Không thể bắt đầu

user-list = Người chơi

lock-room = { $current ->
  [true] Mở phòng
  *[other] Khoá phòng
}
cycle-room = { $current ->
  [true] Vòng lặp
  *[other] Bình thường
}

ready = Sẵn sàng
ready-failed = Không thể sẵn sàng

cancel-ready = Huỷ

room-id = ID phòng: { $id }

download-failed = Không thể tải về Chart

lock-room-failed = Không thể khoá phòng
cycle-room-failed = Không thể thay đổi chế độ

chat-placeholder = Hãy nói gì đó...
chat-send = Gửi
chat-empty = Tin nhắn trống
chat-sent = Đã gửi
chat-send-failed = Không thể gửi tin nhắn

select-chart-host-only = Chỉ chủ phòng mới được chọn Chart
select-chart-local = Không thể chọn Chart đã nhập
select-chart-failed = Không thể chọn Chart
select-chart-not-now = Bạn không thể chọn Chart lúc này

msg-create-room = `{ $user }` đã tạo phòng
msg-join-room = `{ $user }` đã vào phòng
msg-leave-room = `{ $user }` đã rời phòng
msg-new-host = `{ $user }` trở thành chủ phòng
msg-select-chart = Chủ phòng `{ $user }` đã chọn chart `{ $chart }` (#{ $id })
msg-game-start = Chủ phòng `{ $user }` đã bắt đầu, đợi bạn sẵn sàng!
msg-ready = `{ $user }` đã sẵn sàng
msg-cancel-ready = `{ $user }` huỷ sẵn sằng
msg-cancel-game = `{ $user }` huỷ trò chơi
msg-start-playing = Trò chơi bắt đầu!
msg-played = `{ $user }` đã xong: { $score } ({ $accuracy }){ $full-combo ->
  [true] , FC
  *[other] {""}
}
msg-game-end = Trò chơi kết thúc!
msg-abort = `{ $user }` đã buộc huỷ trò chơi
msg-room-lock = { $lock ->
  [true] Đã khoá phòng
  *[other] Đã mở khoá phòng
}
msg-room-cycle = { $cycle ->
  [true] Đã chuyển phòng về chế độ vòng lặp
  *[other] Đã chuyển phòng về chế độ bình thường
}
