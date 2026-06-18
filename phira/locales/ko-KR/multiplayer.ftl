
multiplayer = 멀티플레이어

connect = 연결하기
connect-must-login = 멀티플레이어 모드에 들어가려면 로그인해야 합니다.
connect-success = 연결 성공
connect-failed = 연결 실패
connect-authenticate-failed = 인증 실패
reconnect = 다시 연결 중...

create-room = 방 생성
create-room-success = 방이 생성되었습니다.
create-room-failed = 방 생성 실패
create-invalid-id = 방 ID는 문자, 숫자, -(바), _(언더바)를 포함하여 20자 이하로 구성되어야 합니다.

join-room = 방 참여
join-room-invalid-id = 잘못된 방 ID
join-room-failed = 방 참여 실패

leave-room = 방 나가기
leave-room-failed = 방 나가기 실패

disconnect = 연결 종료

request-start = 게임 시작 요청
request-start-no-chart = 차트를 선택하지 않았습니다.
request-start-failed = 게임 시작 실패

user-list = 유저 목록

lock-room = { $current ->
  [true] 방 잠금 해제
  *[other] 방 잠금
}
cycle-room = { $current ->
  [true] 순환 모드
  *[other] 일반 모드
}

ready = 준비 완료
ready-failed = 준비 실패

cancel-ready = 준비 취소

room-id = 방 ID: { $id }

download-failed = 차트 다운로드 실패

lock-room-failed = 방 잠금 실패
cycle-room-failed = 방 모드 변경 실패

chat-placeholder = 메시지 입력...
chat-send = 보내기
chat-empty = 메시지가 비어 있습니다.
chat-sent = 보냄
chat-send-failed = 메시지 전송 실패

select-chart-host-only = 호스트만 차트를 선택할 수 있습니다.
select-chart-local = 로컬 차트를 선택할 수 없습니다.
select-chart-failed = 차트 선택 실패
select-chart-not-now = 현재 차트를 선택할 수 없습니다.

msg-create-room = `{ $user }` 님이 방을 생성했습니다.
msg-join-room = `{ $user }` 님이 방에 참여했습니다.
msg-leave-room = `{ $user }` 님이 방을 나갔습니다.
msg-new-host = `{ $user }` 님이 새로운 호스트가 되었습니다.
msg-select-chart = 호스트 `{ $user }` 님이 차트 `{ $chart }` (#{ $id })를 선택했습니다.
msg-game-start = 호스트 `{ $user }` 님이 게임을 시작했습니다. 다른 플레이어들은 준비하세요.
msg-ready = `{ $user }` 님이 준비되었습니다.
msg-cancel-ready = `{ $user }` 님이 준비 취소했습니다.
msg-cancel-game = `{ $user }` 님이 게임을 취소했습니다.
msg-start-playing = 게임 시작
msg-played = `{ $user }` 님이 플레이 완료: { $score } ({ $accuracy }){ $full-combo ->
  [true] , 풀 콤보
  *[other] {""}
}
msg-game-end = 게임 종료
msg-abort = `{ $user }` 님이 게임을 중단했습니다.
msg-room-lock = { $lock ->
  [true] 방이 잠겼습니다.
  *[other] 방이 잠금 해제되었습니다.
}
msg-room-cycle = { $cycle ->
  [true] 방이 순환 모드로 변경되었습니다.
  *[other] 방이 일반 모드로 변경되었습니다.
}
