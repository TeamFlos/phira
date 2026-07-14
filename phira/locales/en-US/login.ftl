
login = Login
login-sub = Login to access Phira's online features! (Charts, events, multiplayer, etc.)
back-login = Back to Login
register = Register

email = Email
username = Username
password = Password
forget-password = Forgot password?

name-length-req = Username length should be between { $min } and { $max } characters.
name-has-illegal-char = Username contains unallowed characters.
pwd-length-req = Password length should be between { $min } and { $max } characters.
illegal-email = Unallowed email.

action-success = { $action ->
  [login] Logged in.
  [register] Registered successfully.
  [hykb-login] Logged in with HYKB.
  *[other] _
}
action-failed = { $action ->
  [login] Failed to log in.
  [register] Failed to register.
  [hykb-login] Failed to log in with HYKB.
  *[other] _
}

email-sent = Please check your inbox for an activation email from Phira.

hykb-login = Log in with HYKB (好游快爆)
hykb-login-cancelled = HYKB login cancelled.
login-method-title = Choose login method
login-method-email = Log in with email
login-method-hykb = Log in with HYKB
login-method-recommended = Recommended
hykb-choice-title = Welcome to Phira
hykb-choice-sub =
  This is your first time logging in with your HYKB account.
  If this is your first time playing, choose [Create new player data] to start.
  If you've played before and already have a Phira account, choose [Bind an existing Phira account] to link it.
hykb-choice-register = Create new player data
hykb-choice-claim = Bind an existing Phira account
hykb-reg-name-prompt = Enter your username ({ $min }-{ $max } characters).
