multiplayer = Мультиплеер

connect = Подключиться
connect-must-login = Войдите в аккаунт что бы использовать мультиплеер
connect-success = Подключение успешно
connect-failed = Ошибка подключения
connect-authenticate-failed = Ошибка авторизации

create-room = Создать комнату
create-room-success = Комната создана
create-room-failed = Ошибка создания комнаты

join-room = Присоединиться
join-room-invalid-id = Неверный ID комнаты

leave-room = Покинуть
leave-room-failed = Ошибка при попытке покинуть комнату

disconnect = Отсоединиться

request-start = Начать
request-start-no-chart = Вы не выбрали чарт
request-start-failed = Ошибка при запуске игры

ready = Готов
ready-failed = Ошибка при попытке готовности

cancel-ready = Отмена

room-id = ID комнаты: { $id }

download-failed = Ошибка загрузки чарта

chat-placeholder = Скажите что нибудь…
chat-send = Отправить
chat-empty = Пустое сообщение
chat-sent = Отправлено
chat-send-failed = Ошибка при отправке

select-chart-host-only = Толлко хост может выбирать чарт
select-chart-local = Нельзя выбрать локальный чарт
select-chart-failed = Ошибка при выборе чарта

msg-create-room = `{ $user }` создал комнату
msg-join-room = `{ $user }` присоединился к комнате
msg-leave-room = `{ $user }` покинул комнату
msg-new-host = `{ $user }` стал новым хостос
msg-select-chart = The host `{ $user }` выбрал чарт `{ $chart }` (#{ $id })
msg-game-start = The host `{ $user }` начал игру
msg-ready = `{ $user }` готов
msg-cancel-ready = `{ $user }` отменил готовность
msg-cancel-game = `{ $user }` Отменил игру
msg-start-playing = Игра началась
msg-played = `{ $user }` завершил играть: { $score } ({ $accuracy }){ $full-combo ->
  [true] , Фулл Комбо
  *[other] {""}
}
msg-game-end = Игра окончилась
