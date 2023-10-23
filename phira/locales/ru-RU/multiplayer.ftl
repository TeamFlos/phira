multiplayer = Мультиплеер

connect = Подключиться
connect-must-login = Войдите в аккаунт что бы использовать мультиплеер
connect-success = Подключение успешно
connect-failed = Ошибка подключения
connect-authenticate-failed = Ошибка авторизации

reconnect = Переподключение... 

create-room = Создать комнату
create-room-success = Комната создана
create-room-failed = Ошибка создания комнаты
create-invalid-id = ID комнаты может быть длиной не более 20 символов, и может состоять из строчных и прописных букв, цифр, тире(-) и нижнего подчëркивания. 

join-room = Присоединиться
join-room-invalid-id = Неверный ID комнаты
join-room-failed = Ошибка при попытке присоединиться в комнату

leave-room = Покинуть
leave-room-failed = Ошибка при попытке покинуть комнату

disconnect = Отсоединиться

request-start = Начать
request-start-no-chart = Вы не выбрали чарт
request-start-failed = Ошибка при запуске игры

user-list = Игроки

lock-room = { $current ->
  [true] Закрыть комнату
  *[other] Открыть комнату
}
cycle-room = { $current ->
  [true] По очереди
  *[other] Стандарт
}

ready = Готов
ready-failed = Ошибка при попытке готовности

cancel-ready = Отмена

room-id = ID комнаты: { $id }

download-failed = Ошибка загрузки чарта

lock-room-failed = Ошибка при попытке закрыть комнату
cycle-room-failed = Ошибка при попытке сменить режим комнаты

chat-placeholder = Скажите что нибудь…
chat-send = Отправить
chat-empty = Пустое сообщение
chat-sent = Отправлено
chat-send-failed = Ошибка при отправке

select-chart-host-only = Только хост может выбирать чарт
select-chart-local = Нельзя выбрать локальный чарт
select-chart-failed = Ошибка при выборе чарта
select-chart-not-now = Вы не можете пока что выбрать чарт. 

msg-create-room = `{ $user }` создал комнату
msg-join-room = `{ $user }` присоединился к комнате
msg-leave-room = `{ $user }` покинул комнату
msg-new-host = `{ $user }` стал новым хостом
msg-select-chart = Хост `{ $user }` выбрал чарт `{ $chart }` (#{ $id })
msg-game-start = Хост `{ $user }` начал игру. Остальные игроки, нажмите на кнопку, когда будете готовы. 
msg-ready = `{ $user }` готов
msg-cancel-ready = `{ $user }` отменил готовность
msg-cancel-game = `{ $user }` отменил игру
msg-start-playing = Раунд начался
msg-played = `{ $user }` завершил играть: { $score } ({ $accuracy }){ $full-combo ->
  [true] , Фулл Комбо
  *[other] {""}
}
msg-game-end = Раунд окончен
msg-abort = `{ $user }` вышел посреди раунда
msg-room-lock = { $lock ->
  [true] Комната закрыта
  *[other] Комната открыта
}
msg-room-cycle = { $cycle ->
  [true] Включëн режим "По очереди".Теперь роль хоста передаëтся другому игроку после окончания раунда
  *[other] Включëн режим "Классика" Теперь хост не меняется после окончания раунда
}
