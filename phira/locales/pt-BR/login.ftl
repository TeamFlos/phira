
login = Login
login-sub = Faça login para interagir com a comunidade online ativa
back-login = Voltar ao login
register = Registrar

email = Endereço de email 
username = Nome de usuário
password = Senha

name-length-req = O comprimento do nome de usuário deve estar entre 4 e 20 
name-has-illegal-char = O nome de usuário contém caracteres ilegais 
pwd-length-req = O comprimento da senha deve estar entre 6 e 26 
illegal-email = E-mail ilegal 

action-success = { $action ->
  [login] Conectado com sucesso 
  [register] Registrado com sucesso 
  *[other] _
}
action-failed = { $action ->
  [login] Falha ao fazer login 
  [register] Falha ao registrar 
  *[other] _
}

email-sent = Um e-mail de verificação foi enviado, verifique e faça login 
