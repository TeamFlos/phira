
multiplayer = 多人遊戲

connect = 連線
connect-must-login = 登入後才可進入多人遊戲
connect-success = 連線成功
connect-failed = 連線失敗
connect-authenticate-failed = 身分驗證失敗
reconnect = 斷線重連中…

create-room = 建立房間
create-room-success = 房間已建立
create-room-failed = 建立房間失敗
create-invalid-id = 房間 ID 應由不多於 20 個大小寫英文字母、數字以及 -_ 組成

join-room = 加入房間
join-room-invalid-id = 無效的房間 ID
join-room-failed = 加入房間失敗

leave-room = 離開房間
leave-room-failed = 離開房間失敗

disconnect = 中斷連線

request-start = 開始遊戲
request-start-no-chart = 你尚未選擇譜面
request-start-failed = 開始遊戲失敗

user-list = 使用者列表

lock-room = { $current ->
  [true] 解鎖房間
  *[other] 鎖定房間
}
cycle-room = { $current ->
  [true] 循環模式
  *[other] 普通模式
}

ready = 準備
ready-failed = 準備失敗

cancel-ready = 取消

room-id = 房間 ID：{ $id }

download-failed = 下載譜面失敗

lock-room-failed = 鎖定房間失敗
cycle-room-failed = 切換房間模式失敗

chat-placeholder = 說些什麼…
chat-send = 發送
chat-empty = 訊息內容不能為空
chat-sent = 已發送
chat-send-failed = 訊息發送失敗

select-chart-host-only = 只有房主可以選擇譜面
select-chart-local = 不能選擇本地譜面
select-chart-failed = 選擇譜面失敗
select-chart-not-now = 你現在不能選擇譜面

msg-create-room = `{ $user }` 建立了房間
msg-join-room = `{ $user }` 加入了房間
msg-leave-room = `{ $user }` 離開了房間
msg-new-host = `{ $user }` 成為了新的房主
msg-select-chart = 房主 `{ $user }` 選擇了譜面 `{ $chart }` (#{ $id })
msg-game-start = 房主 `{ $user }` 開始了遊戲，請其他玩家準備
msg-ready = `{ $user }` 已就緒
msg-cancel-ready = `{ $user }` 取消了準備
msg-cancel-game = `{ $user }` 取消了遊戲
msg-start-playing = 遊戲開始
msg-played = `{ $user }` 結束了遊玩：{ $score } ({ $accuracy }){ $full-combo ->
  [true] ，全連
  *[other] {""}
}
msg-game-end = 遊戲結束
msg-abort = `{ $user }` 放棄了遊戲
msg-room-lock = { $lock ->
  [true] 房間已鎖定
  *[other] 房間已解鎖
}
msg-room-cycle = { $cycle ->
  [true] 房間已切換為循環模式
  *[other] 房間已切換為普通模式
}
