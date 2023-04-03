label = 账户

email = 邮箱
username = 用户名
password = 密码

back = 返回
register = 注册
registering = 注册中
login = 登录
logging-in = 登录中
logout = 退出登录
edit-name = 修改名称

not-logged-in = [尚未登录]

logged-out = 退出登录成功

pictrue-read-failed = 无法读取图片
pictrue-load-failed = 无法加载图片
avatar-import-failed = 导入头像失败
avatar-upload-failed = 上传头像失败
avatar-delete-old-failed = 删除原头像失败
avatar-update-failed = 更新头像失败

name-length-req = 用户名长度应介于 4-20 之间
name-has-illegal-char = 用户名包含非法字符
pwd-length-req = 密码长度应介于 6-26 之间
illegal-email = 邮箱不合法

email-sent = 验证信息已发送到邮箱，请验证后登录

action-success = { $action ->
  [login] 登录成功
  [register] 注册成功
  [edit-name] 更新名称成功
  [set-avatar] 更新头像成功
  [update] 更新数据成功
  *[other] _
}
action-failed = { $action ->
  [login] 登录失败
  [register] 注册失败
  [edit-name] 更新名称失败
  [set-avatar] 更新头像失败
  [update] 更新数据失败
  *[other] _
}
