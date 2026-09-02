
info-fail = 加载谱面信息失败
invalid-chart = 无效的谱面

importing = 导入中
import-success = 导入成功
import-failed = 导入失败
import-respack-success = 导入资源包成功
import-respack-failed = 导入资源包失败

batch-import = 批量导入
batch-importing = 批量导入中 ({ $current }/{ $total })
batch-import-confirm = 检测到批量导入数据，是否导入全部 { $count } 张谱面？
batch-import-success = 成功导入 { $count } 个谱面。
batch-import-downloaded-skipped = 已经下载过而跳过的谱面：{ $charts }
batch-import-failed = 批量导入失败
batch-import-failed-chart = 批量导入失败: { $chart }

warning = 警告
warning-new-speed-event = 该谱面使用了 RPE 1.7.0 引入的速度事件缓动。为兼容性考虑，Phira 默认不启用对该事件的支持。如果需要启用，请在谱面信息中勾选“新速度缓动”。
warning-attach-ui = 该谱面使用了 UI 绑定。Phira 最近的版本引入了 UI 绑定的修复，但可能会导致依赖旧行为的谱面出现问题。如有必要，请在谱面信息中取消勾选“UI 绑定修复”。

deeplink-title = 通过链接导入谱面
deeplink-confirm = 是否从链接下载谱面并导入到本地？
deeplink-unofficial = 非官方来源：该文件并非由 Phira 官方服务器（{ $host }）提供，请确认来源可信后再下载
deeplink-download = 下载
deeplink-bad-url = 谱面文件链接不合法
deeplink-downloading = 下载中…
deeplink-dl-failed = 下载失败
deeplink-too-large = 文件大小超出链接导入的上限（100 MiB）
