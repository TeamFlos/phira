
login = Нэвтрэх
login-sub = Идэвхтэй онлайн тоглоомчдын бүлэгт оролцохын тулд нэвтэрнэ үү 
back-login = Нэвтрэх хэсэг рүү буцах
register = Бүртгүүлэх

email = Имэйл хаяг
username = Хэрэглэгчийн нэр
password = Нууц үг

name-length-req = Хэрэглэгчийн нэрийн урт 4-20 тэмдэгт байх ёстой
name-has-illegal-char = Хэрэглэгчийн нэрэнд дэмжигдээгүй үсэг байна
pwd-length-req = Нууц үгийн урт 6-26 тэмдэгт байх ёстой
illegal-email = Имэйл хаяг буруу байна

action-success = { $action ->
  [login] Амжилттай нэвтэрсэн
  [register] Амжилттай бүртгүүлсэн
  *[other] _
}
action-failed = { $action ->
  [login] Нэвтрэхэд амжилтгүй боллоо
  [register] Бүртгүүлэхэд амжилтгүй боллоо
  *[other] _
}

email-sent = Баталгаажуулах имэйл илгээгдсэн. Та баталгаажуулаад нэвтэрнэ үү.