multiplayer = マルチプレイ
connect = 接続
connect-must-login = マルチプレイに参加するにはログインが必要です
connect-success = 接続成功
connect-failed = 接続失敗
connect-authenticate-failed = 認証失敗
reconnect = 再接続中…
create-room = 部屋を作成
create-room-success = 部屋が作成されました
create-room-failed = 部屋の作成に失敗しました
create-invalid-id = 部屋番号は、大文字と小文字の英字、数字、アンダースコア（-_）を合わせて20文字以内で構成します
join-room = 部屋に参加する
join-room-invalid-id = 無効な部屋ID
join-room-failed = 部屋の参加に失敗しました
leave-room = 部屋を出ました
leave-room-failed = 部屋から出れませんでした
disconnect = 切断
request-start = ゲーム開始
request-start-no-chart = まだ譜面を選択していません
request-start-failed = ゲームの開始に失敗しました
user-list = ユーザーリスト
lock-room =
    { $current ->
        [true] 部屋のロックを解除
       *[other] 部屋のロック
    }
cycle-room =
    { $current ->
        [true] ループモード
       *[other] 通常モード
    }
ready = 準備
ready-failed = 準備失敗
cancel-ready = キャンセル
room-id = 部屋ID: { $id }
download-failed = 譜面のダウンロードに失敗しました
lock-room-failed = 部屋のロックに失敗しました
cycle-room-failed = 部屋モードの切り替えに失敗しました
chat-placeholder = なにか言って…
chat-send = 送信
chat-empty = メッセージを空にすることはできません
chat-sent = 送信しました
chat-send-failed = 送信に失敗しました
select-chart-host-only = 譜面を選択できるのは部屋主のみです
select-chart-local = ローカル譜面は選択できません
select-chart-failed = 譜面の選択に失敗しました
select-chart-not-now = 現時点では譜面を選択できません
msg-create-room = `{ $user }` が部屋を作成しました
msg-join-room = `{ $user }` が部屋に参加しました
msg-leave-room = `{ $user }` が部屋を退出しました
msg-new-host = `{ $user }` が新しい部屋主になりました
msg-select-chart = 部屋主`{ $user }` が譜面 `{ $chart }` (#{ $id }) を選択しました
msg-game-start = 部屋主`{ $user }`がゲームを開始しました。他のプレイヤーは準備をしてください
msg-ready = `{ $user }` の準備ができました
msg-cancel-ready = `{ $user }` が準備をキャンセルしました
msg-cancel-game = `{ $user }` がゲームをキャンセルしました
msg-start-playing = ゲーム開始
msg-played =
    `{ $user }` のプレイが終了しました: { $score } ({ $accuracy }){ $full-combo ->
        [true] 、フルコンボ
       *[other] { "" }
    }
msg-game-end = ゲーム終了
msg-abort = `{ $user }` がゲームを諦めました
msg-room-lock =
    { $lock ->
        [true] 部屋が施錠されています
       *[other] 部屋のロックが解除されています
    }
msg-room-cycle =
    { $cycle ->
        [true] ルームがループモードに切り替わりました
       *[other] ルームが通常モードに切り替わりました
    }
