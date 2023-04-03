label = Account

email = Email
username = Username
password = Password

back = Back
register = Register
registering = Registering
login = Login
logging-in = Logging in
logout = Logout
edit-name = Modify name

not-logged-in = [Not logged in]

logged-out = Logged out

pictrue-read-failed = Unable to read the picture
pictrue-load-failed = Unable to load image
avatar-import-failed = Failed to import avatar
avatar-upload-failed = Failed to upload avatar
avatar-delete-old-failed = Failed to delete the original avatar
avatar-update-failed = Failed to update avatar

name-length-req = Username length should be between 4 and 20
name-has-illegal-char = Username contains illegal characters
pwd-length-req = Password length should be between 6 and 26
illegal-email = Illegal email

email-sent = An verification email has been sent, please verify and log in

action-success = { $action ->
  [login] Logged in successfully
  [register] Registered successfully
  [edit-name] Name modified
  [set-avatar] Avatar updated
  [update] Info updated
  *[other] _
}
action-failed = { $action ->
  [login] Failed to log in
  [register] Failed to register
  [edit-name] Failed to modify username
  [set-avatar] Failed to upload avatar
  [update] Failed to update info
  *[other] _
}
