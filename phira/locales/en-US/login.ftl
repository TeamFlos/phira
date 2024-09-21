
login = Login
login-sub = Login to access Phira's online features! (Charts, events, multiplayer, etc.)
back-login = Back to Login
register = Register

email = Email
username = Username
password = Password

name-length-req = Username length should be between 4 and 20 characters.
name-has-illegal-char = Username contains unallowed characters.
pwd-length-req = Password length should be between 6 and 26 characters.
illegal-email = Unallowed email.

action-success = { $action ->
  [login] Logged in.
  [register] Registered successfully.
  *[other] _
}
action-failed = { $action ->
  [login] Failed to log in.
  [register] Failed to register.
  *[other] _
}

email-sent = Please check your inbox for an activation email from Phira.
