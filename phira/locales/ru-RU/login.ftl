login = Войти
login-sub = Войдите для онлайн-доступа Phira (чарты, события, мультиплеер т.д.).
back-login = Назад
register = Регистрация
email = Почта
username = Никнейм
password = Пароль
name-length-req = Длина никнейма не может быть меньше { $min }-x или больше { $max } символов.
name-has-illegal-char = В никнейме есть неподходящие символы.
pwd-length-req = Длина пароля не может быть меньше { $min } или больше { $max } символов.
illegal-email = Неправильно введëн адрес почты.
action-success =
    { $action ->
        [login] Вход успешен
        [register] Регистрация успешна
        [hykb-login] Вход через 3839 Games успешен
       *[other] _
    }
action-failed =
    { $action ->
        [login] Ошибка при входе
        [register] Ошибка при регистрации
        [hykb-login] Ошибка при входе через 3839 Games
       *[other] _
    }
email-sent = На вашу почту пришло верификационное письмо. Подтвердите свою почту, после чего войдите в аккаунт.
forget-password = Забыли пароль?
hykb-login-cancelled = Вход через 3839 Games отменен
login-method-title = Выберите метод входа
login-method-email = Вход через эл.почту
login-method-hykb = Вход через 3839 Games
login-method-recommended = Рекомендовано
hykb-choice-title = Добро пожаловать в Phira
hykb-choice-sub =
    Это ваш первый вход в систему с использованием аккаунта 3839 Games.
    Если вы играете впервые, пожалуйста, выберите «Создать новые данные игрока», чтобы начать игру.
    Если вы играли раньше и у вас уже есть аккаунт Phira, пожалуйста, выберите «Привязать существующий аккаунт Phira», чтобы привязать ее.
hykb-choice-register = Создать новые данные игрока
hykb-choice-claim = Привязать существующий аккаунт Phira
hykb-reg-name-prompt = Введите псевдоним ({ $min }-{ $max } символов)
hykb-reg-name-confirm = Подтвердить
hykb-other-login-not-bound = Данный аккаунт не привязан к аккаунту 3839 Games, вход невозможен.
pending-delete-title = Ожидающий запрос на удаление
pending-delete-message = Ваш аккаунт имеет ожидающий запрос на удаление. Хотите продолжить вход для отмены?
hykb-other-login-notice = Только аккаунт, привязанный к 3839 Games может быть использован для входа.
