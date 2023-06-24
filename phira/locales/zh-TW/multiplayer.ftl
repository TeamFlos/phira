
multiplayer = 多人遊戲

connect = 連線
connect-must-login = 登入後才能進入多人遊戲
connect-success = 連線成功
connect-failed = 連線失敗
connect-authenticate-failed = 身分驗證失敗

create-room = 創建房間
create-room-success = 房間已創建
create-room-failed = 創建房間失敗

join-room = 加入房間
join-room-invalid-id = 無效的房間 ID

leave-room = 離開房間
leave-room-failed = 離開房間失敗

disconnect = 中斷連線

request-start = 開始遊戲
request-start-no-chart = 你尚未選擇譜面
request-start-failed = 開始遊戲失敗

ready = 準備
ready-failed = 準備失敗

cancel-ready = 取消

room-id = 房間 ID：{ $id }

download-failed = 下載譜面失敗

chat-placeholder = 說些什麼…
chat-send = 發送
chat-empty = 訊息內容不能為空
chat-sent = 已發送
chat-send-failed = 訊息發送失敗

select-chart-host-only = 只有房主可以選擇譜面
select-chart-local = 不能選擇本地譜面
select-chart-failed = 選擇譜面失敗

msg-create-room = `{ $user }` 創建了房間
msg-join-room = `{ $user }` 加入了房間
msg-leave-room = `{ $user }` 離開了房間
msg-new-host = `{ $user }` 成為了新的房主
msg-select-chart = 房主 `{ $user }` 選擇了譜面 `{ $chart }` (#{ $id })
msg-game-start = 房主 `{ $user }` 開始了遊戲
msg-ready = `{ $user }` 已就緒
msg-cancel-ready = `{ $user }` 取消了準備
msg-cancel-game = `{ $user }` 取消了遊戲
msg-start-playing = 遊戲開始
msg-played = `{ $user }` 結束了遊玩：{ $score } ({ $accuracy }){ $full-combo ->
  [true] ，全連
  *[other] {""}
}
msg-game-end = 遊戲結束
