
login = Войти
login-sub = Войдите что-бы взаимодействовать с сообществом. 
back-login = Вернуться
register = Регистрация

email = Почта
username = Никнейм
password = Пароль

name-length-req = Длина никнейма не может быть меньше 4-х или больше 20-ти символов
name-has-illegal-char = В никнейме есть неподходящие символы
pwd-length-req = Длина пароля не может быть меньше 6-ти или больше 20-ти символов
illegal-email = Неправильно введëн адрес почты

action-success = { $action ->
  [login] Успешный вход
  [register] Успешная регистрация
  *[other] _
}
action-failed = { $action ->
  [login] Ошибка при входе
  [register] Ошибка при регистрации
  *[other] _
}

email-sent = На вашу почту пришло верификационное письмо. Подтердите свою почту, после чего войдите в аккаунт
