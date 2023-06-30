
multiplayer = 多人游戏

connect = 连接
connect-must-login = 登录后才能进入多人游戏
connect-success = 连接成功
connect-failed = 连接失败
connect-authenticate-failed = 鉴权失败
reconnect = 断线重连中…

create-room = 创建房间
create-room-success = 房间已创建
create-room-failed = 创建房间失败
create-invalid-id = 房间 ID 由不多于 20 个大小写英文字母、数字以及 -_ 组成

join-room = 加入房间
join-room-invalid-id = 无效的房间 ID
join-room-failed = 加入房间失败

leave-room = 离开房间
leave-room-failed = 离开房间失败

disconnect = 断开连接

request-start = 开始游戏
request-start-no-chart = 你还没有选择谱面
request-start-failed = 开始游戏失败

user-list = 用户列表

lock-room = { $current ->
  [true] 解锁房间
  *[other] 锁定房间
}
cycle-room = { $current ->
  [true] 循环模式
  *[other] 普通模式
}

ready = 准备
ready-failed = 准备失败

cancel-ready = 取消

room-id = 房间 ID：{ $id }

download-failed = 下载谱面失败

lock-room-failed = 锁定房间失败
cycle-room-failed = 切换房间模式失败

chat-placeholder = 说些什么…
chat-send = 发送
chat-empty = 消息不能为空
chat-sent = 已发送
chat-send-failed = 消息发送失败

select-chart-host-only = 只有房主可以选择谱面
select-chart-local = 不能选择本地谱面
select-chart-failed = 选择谱面失败
select-chart-not-now = 你现在不能选择谱面

msg-create-room = `{ $user }` 创建了房间
msg-join-room = `{ $user }` 加入了房间
msg-leave-room = `{ $user }` 离开了房间
msg-new-host = `{ $user }` 成为了新的房主
msg-select-chart = 房主 `{ $user }` 选择了谱面 `{ $chart }` (#{ $id })
msg-game-start = 房主 `{ $user }` 开始了游戏，请其他玩家准备
msg-ready = `{ $user }` 已就绪
msg-cancel-ready = `{ $user }` 取消了准备
msg-cancel-game = `{ $user }` 取消了游戏
msg-start-playing = 游戏开始
msg-played = `{ $user }` 结束了游玩：{ $score } ({ $accuracy }){ $full-combo ->
  [true] ，全连
  *[other] {""}
}
msg-game-end = 游戏结束
msg-abort = `{ $user }` 放弃了游戏
msg-room-lock = { $lock ->
  [true] 房间已锁定
  *[other] 房间已解锁
}
msg-room-cycle = { $cycle ->
  [true] 房间已切换为循环模式
  *[other] 房间已切换为普通模式
}
