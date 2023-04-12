
login = Login
login-sub = Login เพื่อใช้โหมด Online
back-login = กลับไป Login
register = สมัคร

email = Email
username = ชื่อผู้ใช้
password = รหัสผ่าน

name-length-req = ชื่อควรยาว 4-20 ตัว
name-has-illegal-char = ชื่อผู้ใช้มีตัวที่ไม่อนุญาตให้ใช้
pwd-length-req = รหัสผ่านควรยาว 6-26 ตัว
illegal-email = Illegal email

action-success = { $action ->
  [login] Login เรียบร้อย
  [register] สมัครเรียบร้อย
  *[other] _
}
action-failed = { $action ->
  [login] ไม่สามารถ login ได้
  [register] ไม่สามารถสมัครได้
  *[other] _
}

email-sent = รหัสยืนยันถูกส่งไปยัง email เรียบร้อย, กรุณายืนยันตัวตนเพื่อ login