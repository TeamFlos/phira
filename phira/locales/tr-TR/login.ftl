login = Giriş Yap
login-sub = Aktif çevrimiçi topluluğumuzla etkileşmek için giriş yap
back-login = Girişe Dön
register = Kaydol
email = E-Posta Adresi
username = Kullanıcı Adı
password = Şifre
name-length-req = Kullanıcı adı uzunluğu { $min } ile { $max } karakter arasında olmalıdır
name-has-illegal-char = Kullanıcı adı geçersiz karakterler içeriyor
pwd-length-req = Şifre uzunluğu { $min } ile { $max } karakter arasında olmalıdır
illegal-email = Geçersiz E-Posta
action-success =
    { $action ->
        [login] Başarıyla giriş yapıldı
        [register] Başarıyla kaydolundu
       *[other] _
    }
action-failed =
    { $action ->
        [login] Giriş başarısız
        [register] Kayıt başarısız
       *[other] _
    }
email-sent = Bir doğrulama e-postası gönderildi. Lütfen hesabınızı doğrulayıp giriş yapın.
