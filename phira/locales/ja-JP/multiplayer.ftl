
multiplayer = マルチプレイヤー

connect = 接続
connect-must-login = ログイン後にマルチプレイヤー利用可能
connect-success = 接続成功
connect-failed = 接続失敗
connect-authenticate-failed = 身分認証失敗
reconnect = 再接続中…

create-room = ルームを作る
create-room-success = ルームが作った
create-room-failed = ルーム作る失敗
create-invalid-id = ルーム番号は２０個の英語の文字、数字、下線で構成できる

join-room = ルームを入る
join-room-invalid-id = 無効的なルーム番号
join-room-failed = ルームを入る失敗

leave-room = ルームを離れる
leave-room-failed = ルームを離れる失敗

disconnect = 接続中断

request-start = ゲームはじめ
request-start-no-chart = 譜面まだ選ばない
request-start-failed = ゲーム始める失敗

user-list = 使用者リスト

lock-room = { $current ->
  [true] ルームはアンロックされった
  *[other] ルームはロックされた
}
cycle-room = { $current ->
  [true] サイクルモード
  *[other] 普通モード
}

ready = 準備
ready-failed = 準備失敗

cancel-ready = キャンセル

room-id = ルーム番号：{ $id }

download-failed = 譜面ダウンロード失敗

lock-room-failed = ルームをロック失敗
cycle-room-failed = ルームのスイッチ失敗

chat-placeholder = 何か言う…
chat-send = 送信
chat-empty = 送信内容は空にできません
chat-sent = 送信した
chat-send-failed = 送信失敗

select-chart-host-only = 所有者だけ譜面を選べます
select-chart-local = ローカル譜面選べません
select-chart-failed = 譜面選ぶ失敗
select-chart-not-now = 貴方は今譜面を選べません

msg-create-room = `{ $user }`はルームを作りました
msg-join-room = `{ $user }`は入りました
msg-leave-room = `{ $user }`は離れました
msg-new-host = `{ $user }`は新所有者になりました
msg-select-chart = 所有者`{ $user }`は`{ $chart }` (#{ $id })の譜面を選びました
msg-game-start = 所有者`{ $user }`はゲームを始まりました。みんなで準備してください
msg-ready = `{ $user }`準備された
msg-cancel-ready = `{ $user }`は準備をキャンセルしました
msg-cancel-game = `{ $user }`はゲームをキャンセルしました
msg-start-playing = ゲームはじめ
msg-played = `{ $user }` 結束了遊玩：{ $score } ({ $accuracy }){ $full-combo ->
  [true] ，全連
  *[other] {""}
}
msg-game-end = ゲーム終わり
msg-abort = `{ $user }`はゲームを放棄しました
msg-room-lock = { $lock ->
  [true] ルームはロックされた
  *[other] ルームはアンロックされった
}
msg-room-cycle = { $cycle ->
  [true] ルームはサイクルモードにスイッチされた
  *[other] ルームは普通モードにスイッチされた
}
