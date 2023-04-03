
login = Login
login-sub = Login to engage with the active online community
back-login = Back to Login
register = Register

email = Email address
username = Username
password = Password

name-length-req = Username length should be between 4 and 20
name-has-illegal-char = Username contains illegal characters
pwd-length-req = Password length should be between 6 and 26
illegal-email = Illegal email

action-success = { $action ->
  [login] Logged in successfully
  [register] Registered successfully
  *[other] _
}
action-failed = { $action ->
  [login] Failed to log in
  [register] Failed to register
  *[other] _
}

