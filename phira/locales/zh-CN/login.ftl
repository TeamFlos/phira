
login = 登录
login-sub = 登录以加入活跃的在线社区
back-login = 返回登录
register = 注册

email = 电子邮箱
username = 用户名
password = 密码
forget-password = 忘记密码

name-length-req = 用户名长度应介于 { $min }-{ $max } 之间
name-has-illegal-char = 用户名包含非法字符
pwd-length-req = 密码长度应介于 { $min }-{ $max } 之间
illegal-email = 邮箱不合法

action-success = { $action ->
  [login] 登录成功
  [register] 注册成功
  [hykb-login] 好游快爆登录成功
  *[other] _
}
action-failed = { $action ->
  [login] 登录失败
  [register] 注册失败
  [hykb-login] 好游快爆登录失败
  *[other] _
}

email-sent = 验证信息已发送到邮箱，请验证后登录

hykb-login-cancelled = 已取消好游快爆登录
login-method-title = 选择登录方式
login-method-email = 邮箱登录
login-method-hykb = 好游快爆登录
login-method-recommended = 推荐
hykb-choice-title = 欢迎来到 Phira
hykb-choice-sub =
  这是你首次使用快爆账号进行登录。
  若您是第一次游玩，请选择【创建新的玩家数据】进行游戏。
  若您曾经游玩过且已拥有Phira账号，请选择【绑定已有Phira账号】进行绑定操作。
hykb-choice-register = 创建新的玩家数据
hykb-choice-claim = 绑定已有Phira账号
hykb-reg-name-prompt = 请输入你的用户名（{ $min }-{ $max } 位）
hykb-reg-name-confirm = 确认
hykb-other-login-notice = 只有绑定了好游快爆的账号才可以登录游戏。
hykb-other-login-not-bound = 该账号未绑定好游快爆，无法登录
hykb-bind-required-title = 需要绑定好游快爆
hykb-bind-required = 该账号尚未绑定好游快爆，需要绑定后才能登录游戏。
hykb-bind-required-confirm = 绑定好游快爆
