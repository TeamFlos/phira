
multiplayer = Đa Người Chơi

connect = Kết nối
connect-must-login = Cần đăng nhập để vào chế độ đa người chơi
connect-success = Kết nối thành công
connect-failed = Kết nối thất bại
connect-authenticate-failed = Ủy quyền thất bại
reconnect = Đang kết nối lại...

create-room = Tạo phòng
create-room-success = Đã tạo phòng
create-room-failed = Tạo phòng thất bại
create-invalid-id = ID phòng bao gồm không quá 20 ký tự, bao gồm chữ cái, số, - (gạch ngang) và _ (gạch dưới)


join-room = Vào phòng
join-room-invalid-id = ID phòng không tồn tại
join-room-failed = Không thể tham gia phòng

leave-room = Rời phòng
leave-room-failed = Rời phòng thất bại (lmao)

disconnect = Ngắt kết nối

request-start = Bắt đầu Game
request-start-no-chart = Bạn chưa chọn biểu đồ
request-start-failed = Bắt đầu thất bại

user-list = Người Chơi

lock-room = { $current ->
  [true] Mở phòng
  *[other] Đóng phòng
}
cycle-room = { $current ->
  [true] Chế độ đạp xe (CM)
  *[other] Chế độ thường
}


ready = Sẵn sàng
ready-failed = Không thể sẵn sàng

cancel-ready = Hủy bỏ

room-id = ID Phòng: { $id }

download-failed = Không thể tải biểu đồ

chat-placeholder = Viết gì đó
chat-send = Gửi
chat-empty = Tin nhắn rỗng
chat-sent = Đã gửi
chat-send-failed = Không thể gửi

select-chart-host-only = Chỉ chủ phòng mới có quyền chọn
select-chart-local = Không chọn biểu đồ nội bộ
select-chart-failed = Chọn biểu đồ thất bại
select-chart-not-now = Bạn không thể chọn biểu đồ ngay bây giờ. 

msg-create-room = `{ $user }` đã tạo phòng
msg-join-room = `{ $user }` đã tham gia
msg-leave-room = `{ $user }` đã rời phòng
msg-new-host = `{ $user }` trở thành chủ mới
msg-select-chart = Chủ phòng `{ $user }`  chọn biểu đồ  `{ $chart }` (#{ $id })
msg-game-start = Chủ phòng `{ $user }` bắt đầu trò chơi. Những người chơi khác nên sẵn sàng.
msg-ready = `{ $user }` đã sẵn sàng
msg-cancel-ready = `{ $user }` hủy sẵn sàng
msg-cancel-game = `{ $user }` hủy trò chơi 
msg-start-playing = Trò chơi bắt đầu
msg-played = `{ $user }` chơi xong: { $score } ({ $accuracy }){ $full-combo ->
  [true] , full combo
  *[other] {""}
}
msg-game-end = Trò chơi kết thúc
msg-abort = `{ $user }` hủy bỏ trò chơi
msg-room-lock = { $lock ->
  [true] Đã khoá phòng
  *[other] Đã mở khoá phòng
}
msg-room-cycle = { $cycle ->
  [true] Phòng đã đổi sang chế độ đi xe đạp (CM)
  *[other] Phòng đã chuyển sang chế độ bình thường
}

