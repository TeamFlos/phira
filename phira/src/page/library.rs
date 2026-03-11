prpr_l10n::tl_file!("library");

use super::{CollectionPage, FavoritesPage, NextPage, Page, SharedState};
use crate::{
    charts_view::{ChartDisplayItem, ChartsView, NEED_UPDATE},
    client::{recv_raw, Chart, ChartRef, Client, Collection, LocalCollection},
    dir, get_data, get_data_mut,
    icons::Icons,
    page::{favorites::FAV_PAGE_RESULT, ChartItem},
    popup::Popup,
    rate::RateDialog,
    save_data,
    scene::{check_read_tos_and_policy, compress_folder, confirm_dialog, ChartOrder, JUST_LOADED_TOS},
    tabs::{Tabs, TitleFn},
    tags::TagsDialog,
};
use anyhow::{anyhow, Error, Result};
use chrono::{DateTime, Utc};
use inputbox::InputBox;
use macroquad::prelude::*;
use prpr::{
    ext::{poll_future, semi_black, JoinToString, LocalTask, RectExt, SafeTexture, ScaleType},
    scene::{request_file, request_input, return_input, show_error, show_message, take_input, NextScene},
    task::Task,
    ui::{button_hit, DRectButton, Dialog, RectButton, Ui},
};
use serde::{Deserialize, Serialize};
use std::{
    any::Any,
    borrow::Cow,
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs::File,
    io::{self, BufWriter, Write},
    mem,
    ops::Deref,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc, Arc, Mutex,
    },
};
use tap::Tap;

pub static FAV_UPDATED: AtomicBool = AtomicBool::new(false);
pub static CHOOSE_COVER: AtomicBool = AtomicBool::new(false);

thread_local! {
    pub static CHOSEN_COVER: RefCell<Option<Result<i32, String>>> = const { RefCell::new(None) };
}

const PAGE_NUM: u64 = 28;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChartListType {
    Local,
    Ranked,
    Special,
    Unstable,
    Popular,
}

struct ChartList {
    ty: ChartListType,
    view: ChartsView,
}
impl ChartList {
    fn new(ty: ChartListType, icons: Arc<Icons>, rank_icons: [SafeTexture; 8]) -> Self {
        let mut view = ChartsView::new(icons, rank_icons);
        view.can_refresh = ty != ChartListType::Local;
        Self { ty, view }
    }
}

struct CreateFavorite {
    name: String,
    charts: Vec<ChartRef>,
}

type OnlineTaskResult = (Vec<ChartDisplayItem>, Vec<Chart>, u64);
type OnlineTask = Task<Result<OnlineTaskResult>>;

pub struct LibraryPage {
    tabs: Tabs<ChartList>,

    current_page: u64,
    online_total_page: u64,
    prev_page_btn: DRectButton,
    next_page_btn: DRectButton,

    online_task: Option<OnlineTask>,

    icons: Arc<Icons>,
    rank_icons: [SafeTexture; 8],

    import_btn: DRectButton,

    search_btn: DRectButton,
    search_str: String,
    search_clr_btn: RectButton,

    order_btn: DRectButton,
    order_menu: Popup,
    order_menu_options: Vec<ChartOrder>,
    need_show_order_menu: bool,
    current_order: ChartOrder,
    order_meta_menu: Popup,
    need_show_order_meta_menu: bool,

    order_rev: bool,

    filter_btn: DRectButton,
    tags: TagsDialog,
    tags_last_show: bool,
    rating: RateDialog,
    rating_last_show: bool,
    filter_show_tag: bool,

    // 收藏夹 || Favorites
    fav_btn: DRectButton,
    // None = 显示全部 || show all,      Some(folder_name) = 过滤指定收藏夹 || filter by folder
    current_fav_index: Option<usize>,
    sync_fav_task: Option<Task<Result<Option<Collection>>>>,
    force_sync_to_cloud: Arc<AtomicBool>,

    multi_operation_btn: DRectButton,
    multi_operation_menu: Popup,
    multi_operation_options: Vec<&'static str>,
    need_show_multi_operation_menu: bool,

    multi_select_btn: DRectButton,
    multi_select_menu: Popup,
    need_show_multi_select_menu: bool,

    multi_select_cancel_btn: DRectButton,
    delete_multi: Arc<AtomicBool>,
    multi_create_fav_task: Option<Task<Result<CreateFavorite>>>,

    next_page: Option<NextPage>,
    next_page_task: LocalTask<Result<NextPage>>,

    export_paths: Option<Vec<String>>,
    export_task: Option<mpsc::Receiver<Result<()>>>,
    export_progress: Arc<AtomicU32>,
    export_total: usize,
}

impl LibraryPage {
    pub fn new(icons: Arc<Icons>, rank_icons: [SafeTexture; 8]) -> Result<Self> {
        NEED_UPDATE.store(true, Ordering::Relaxed);
        let icon_star = icons.star.clone();
        let new_list = |ty| ChartList::new(ty, Arc::clone(&icons), rank_icons.clone());
        Ok(Self {
            tabs: Tabs::new([
                (new_list(ChartListType::Local), || tl!("local")),
                (new_list(ChartListType::Ranked), || ttl!("chart-ranked")),
                (new_list(ChartListType::Special), || ttl!("chart-special")),
                (new_list(ChartListType::Unstable), || ttl!("chart-unstable")),
                (new_list(ChartListType::Popular), || tl!("popular")),
            ] as [(ChartList, TitleFn); 5]),

            current_page: 0,
            online_total_page: 0,
            prev_page_btn: DRectButton::new(),
            next_page_btn: DRectButton::new(),

            online_task: None,

            icons,
            rank_icons,

            import_btn: DRectButton::new(),

            search_btn: DRectButton::new(),
            search_str: String::new(),
            search_clr_btn: RectButton::new(),

            order_btn: DRectButton::new(),
            order_menu: Popup::new().with_size(0.5),
            order_menu_options: Vec::new(),
            need_show_order_menu: false,
            current_order: ChartOrder::Default,
            order_meta_menu: Popup::new().with_size(0.5),
            need_show_order_meta_menu: false,

            order_rev: true,

            filter_btn: DRectButton::new(),
            tags: TagsDialog::new(true).tap_mut(|it| it.perms = get_data().me.as_ref().map(|it| it.perms()).unwrap_or_default()),
            tags_last_show: false,
            rating: RateDialog::new(icon_star, true).tap_mut(|it| {
                it.rate.score = 3;
                it.rate_upper.as_mut().unwrap().score = 10;
            }),
            rating_last_show: false,
            filter_show_tag: true,

            fav_btn: DRectButton::new(),
            current_fav_index: None,
            sync_fav_task: None,
            force_sync_to_cloud: Arc::default(),

            multi_operation_btn: DRectButton::new(),
            multi_operation_menu: Popup::new().with_size(0.5),
            multi_operation_options: Vec::new(),
            need_show_multi_operation_menu: false,

            multi_select_btn: DRectButton::new(),
            multi_select_menu: Popup::new()
                .with_size(0.5)
                .with_options(vec![tl!("multi-select-all").into_owned(), tl!("multi-select-invert").into_owned()]),
            need_show_multi_select_menu: false,

            multi_select_cancel_btn: DRectButton::new(),
            delete_multi: Arc::default(),
            multi_create_fav_task: None,

            next_page: None,
            next_page_task: None,

            export_paths: None,
            export_task: None,
            export_progress: Arc::default(),
            export_total: 0,
        })
    }
}

impl LibraryPage {
    fn total_page(&self) -> u64 {
        if self.tabs.selected().ty == ChartListType::Local {
            0
        } else {
            self.online_total_page
        }
    }

    pub fn load_online(&mut self) {
        if get_data().config.offline_mode {
            show_message(tl!("offline-mode")).error();
            return;
        }
        if get_data().me.is_none() {
            show_error(anyhow!(tl!("must-login")));
            return;
        }
        if !check_read_tos_and_policy(false, false) {
            return;
        }
        self.tabs.selected_mut().view.reset_scroll();
        self.tabs.selected_mut().view.clear();
        let page = self.current_page;
        let search = self.search_str.clone();
        let order = {
            let order = match self.current_order {
                ChartOrder::Default => "updated",
                ChartOrder::Name => "name",
                ChartOrder::Rating => "rating",
                ChartOrder::Difficulty => "difficulty",
            };
            if self.order_rev {
                format!("-{order}")
            } else {
                order.to_owned()
            }
        };
        let tags = self
            .tags
            .tags
            .tags()
            .iter()
            .cloned()
            .chain(self.tags.unwanted.as_ref().unwrap().tags().iter().map(|it| format!("-{it}")))
            .join(",");
        let division = self.tags.division;
        let rating_range = format!("{},{}", self.rating.rate.score as f32 / 10., self.rating.rate_upper.as_ref().unwrap().score as f32 / 10.);
        let chosen = self.tabs.selected().ty;
        let popular = chosen == ChartListType::Popular;
        let typ = match chosen {
            ChartListType::Ranked => 0,
            ChartListType::Special => 1,
            ChartListType::Unstable => 2,
            _ => -1,
        };
        let by_me = if self.tags.show_me {
            get_data().me.as_ref().map(|it| it.id)
        } else {
            None
        };
        let show_unreviewed = self.tags.show_unreviewed;
        let show_stabilize = self.tags.show_stabilize;
        self.online_task = Some(Task::new(async move {
            let mut q = Client::query::<Chart>();
            if popular {
                q = q.suffix("/popular");
            } else {
                q = q.search(search).order(order).tags(tags).query("rating", rating_range);
            }
            if let Some(me) = by_me {
                q = q.query("uploader", me.to_string());
            }
            if show_stabilize {
                q = q.query("stableRequest", "true");
            } else if show_unreviewed {
                q = q.query("reviewed", "false").query("stableRequest", "false");
            }
            let (remote_charts, count) = q
                .query("type", typ.to_string())
                .query("division", division)
                .page(page)
                .page_num(PAGE_NUM)
                .send()
                .await?;
            let total_page = if count == 0 { 0 } else { (count - 1) / PAGE_NUM + 1 };
            let charts: Vec<_> = remote_charts.iter().map(ChartDisplayItem::from_remote).collect();
            Ok((charts, remote_charts, total_page))
        }));
    }

    fn sync_local(&mut self, s: &SharedState) {
        let mut charts_local = s.charts_local.iter().collect::<Vec<_>>();
        self.current_order.apply(&mut charts_local, |it| it);
        if self.order_rev {
            charts_local.reverse();
        }

        let search_by_id = if let Some(id_str) = self.search_str.strip_prefix('#') {
            id_str.trim().parse::<i32>().ok()
        } else {
            None
        };
        let local_matcher = |chart: &ChartItem| {
            if let Some(search_id) = search_by_id {
                chart.info.id == Some(search_id)
            } else {
                chart.info.name.contains(&self.search_str)
            }
        };

        let list = self.tabs.selected_mut();
        if list.ty == ChartListType::Local {
            let mut charts = Vec::new();
            if let Some(fav_index) = self.current_fav_index {
                charts.extend(get_data().collection_by_index(fav_index).charts.iter().filter_map(|it| {
                    match it {
                        ChartRef::Online(_, chart) => {
                            let chart = chart.as_ref().unwrap();
                            search_by_id
                                .map_or_else(|| chart.name.contains(&self.search_str), |search_id| chart.id == search_id)
                                .then(|| ChartDisplayItem::from_remote(chart))
                        }
                        ChartRef::Local(path) => charts_local
                            .iter()
                            .find(|it| it.local_path.as_ref().is_some_and(|its_path| its_path == path) && local_matcher(it))
                            .map(|it| ChartDisplayItem::new(Some((*it).clone()), None)),
                    }
                }))
            } else {
                charts.push(ChartDisplayItem::new(None, None));
                charts.extend(
                    charts_local
                        .iter()
                        .filter(|it| local_matcher(it))
                        .map(|it| ChartDisplayItem::new(Some((*it).clone()), None)),
                )
            }
            list.view.set(s.t, charts);
        }
    }

    fn on_order_update(&mut self, s: &mut SharedState) {
        let list = self.tabs.selected_mut();
        if list.ty == ChartListType::Local {
            self.sync_local(s);
        } else {
            self.current_page = 0;
            self.load_online();
        }
    }

    fn check_fav_page(&mut self, s: &mut SharedState) {
        if let Some(result) = FAV_PAGE_RESULT.with(|it| it.borrow_mut().take()) {
            self.current_fav_index = result;
            self.sync_local(s);
        }
    }

    fn update_order_meta_menu_options(&mut self) {
        self.order_meta_menu.set_options(vec![
            tl!("order-by", "order" => self.order_menu_options[self.current_order as usize].label()),
            if self.order_rev { tl!("order-desc") } else { tl!("order-asc") }.into(),
        ]);
    }
}

struct ExportConfig {
    file: File,
    deleter: Box<dyn FnOnce() -> io::Result<()> + Send>,
}

#[derive(Serialize, Deserialize)]
pub struct ExportInfo {
    pub exported_at: DateTime<Utc>,
    pub version: String,
}

static EXPORT_CONFIG: Mutex<Option<io::Result<ExportConfig>>> = Mutex::new(None);
#[cfg(target_os = "ios")]
static EXPORT_PICKER_PATH: Mutex<Option<String>> = Mutex::new(None);

#[cfg(target_os = "ios")]
fn present_export_picker(path: String) {
    use objc2::{available, define_class, rc::Retained, runtime::ProtocolObject, MainThreadMarker, MainThreadOnly};
    use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSString, NSURL};
    use objc2_ui_kit::{UIDocumentPickerDelegate, UIDocumentPickerViewController};

    thread_local! {
        static DELEGATE: RefCell<Option<Retained<PickerDelegate>>> = const { RefCell::new(None) };
    }

    define_class! {
        // SAFETY:
        // - The superclass NSObject does not have any subclassing requirements.
        // - `PickerDelegate` does not implement `Drop`.
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        struct PickerDelegate;

        // SAFETY: `NSObjectProtocol` has no safety requirements.
        unsafe impl NSObjectProtocol for PickerDelegate {}

        // SAFETY: `UIDocumentPickerDelegate` has no safety requirements.
        unsafe impl UIDocumentPickerDelegate for PickerDelegate {
            // SAFETY: The signature is correct.
            #[unsafe(method(documentPicker:didPickDocumentsAtURLs:))]
            fn did_pick_documents_at_urls(&self, _controller: &UIDocumentPickerViewController, _urls: &NSArray<NSURL>) {
                show_message(tl!("multi-exported")).ok();
            }
        }
    }

    impl PickerDelegate {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(());
            unsafe { objc2::msg_send![super(this), init] }
        }
    }

    let mtm = MainThreadMarker::new().unwrap();

    let url = NSURL::fileURLWithPath(&NSString::from_str(&path));
    let urls = NSArray::from_retained_slice(&[url]);
    let picker = UIDocumentPickerViewController::alloc(mtm);
    let picker = if available!(ios = 14.0.0) {
        UIDocumentPickerViewController::initForExportingURLs_asCopy(picker, &urls, true)
    } else {
        #[allow(deprecated)]
        {
            use objc2_ui_kit::UIDocumentPickerMode;
            UIDocumentPickerViewController::initWithURLs_inMode(picker, &urls, UIDocumentPickerMode::ExportToService)
        }
    };
    let dlg_obj = PickerDelegate::new(mtm);
    picker.setDelegate(Some(ProtocolObject::from_ref(&*dlg_obj)));
    DELEGATE.with(|it| *it.borrow_mut() = Some(dlg_obj));

    if let Some(controller) = inputbox::backend::IOS::get_top_view_controller(mtm) {
        controller.presentViewController_animated_completion(&picker, true, None);
    } else {
        show_error(Error::msg("Failed to present export dialog"));
    }
}

fn request_export() {
    let suggested_name = format!("phira-export-{}.zip", chrono::Local::now().format("%Y%m%d-%H%M%S"));
    cfg_if::cfg_if! {
        if #[cfg(target_os = "android")] {
            unsafe {
                let env = miniquad::native::attach_jni_env();
                let ctx = ndk_context::android_context().context();
                let class = (**env).GetObjectClass.unwrap()(env, ctx);
                let method =
                    (**env).GetMethodID.unwrap()(env, class, c"showExportDialog".as_ptr() as _, c"(Ljava/lang/String;)V".as_ptr() as _);
                let url = std::ffi::CString::new(suggested_name).unwrap();
                (**env).CallVoidMethod.unwrap()(
                    env,
                    ctx,
                    method,
                    (**env).NewStringUTF.unwrap()(env, url.as_ptr()),
                );
            }
        } else if #[cfg(target_os = "ios")] {
            use objc2_foundation::NSTemporaryDirectory;

            let dir = NSTemporaryDirectory();
            let output_path = PathBuf::from(dir.to_string()).join(&suggested_name);
            let output_path_str = output_path.to_string_lossy().to_string();
            let config = File::create(&output_path).map(|file| {
                let delete_path = output_path.clone();
                ExportConfig {
                    file,
                    deleter: Box::new(move || std::fs::remove_file(delete_path)),
                }
            });
            if config.is_ok() {
                EXPORT_PICKER_PATH.lock().unwrap().replace(output_path_str);
            }
            EXPORT_CONFIG.lock().unwrap().replace(config);
        } else {
            if let Some(output_path) = rfd::FileDialog::new().set_title(tl!("multi-export-title")).set_file_name(&suggested_name).save_file() {
                let config = File::create(&output_path).map(|file| ExportConfig {
                    file,
                    deleter: Box::new(move || std::fs::remove_file(output_path)),
                });
                EXPORT_CONFIG.lock().unwrap().replace(config);
            }
        }
    }
}

#[cfg(target_os = "android")]
fn delete_uri(java_vm: jni::JavaVM, uri: jni::objects::GlobalRef) {
    let mut env = java_vm.attach_current_thread().unwrap();
    let ctx = ndk_context::android_context().context();
    let ctx = unsafe { jni::objects::JObject::from_raw(ctx as _) };
    env.call_method(ctx, "deleteUri", "(Landroid/net/Uri;)V", &[(&uri).into()]).unwrap();
}

#[cfg(target_os = "android")]
#[export_name = "Java_quad_1native_QuadNative_processExportFd"]
extern "system" fn process_export_fd(env: jni::JNIEnv, _: jni::objects::JClass, uri: jni::objects::JObject, fd: jni::sys::jint) {
    use std::os::fd::FromRawFd;
    let java_vm = env.get_java_vm().unwrap();
    let uri = env.new_global_ref(uri).unwrap();
    let file = unsafe { File::from_raw_fd(fd as _) };
    EXPORT_CONFIG.lock().unwrap().replace(Ok(ExportConfig {
        file,
        deleter: Box::new(|| {
            delete_uri(java_vm, uri);
            Ok(())
        }),
    }));
}

impl Page for LibraryPage {
    fn label(&self) -> Cow<'static, str> {
        tl!("label")
    }

    fn enter(&mut self, s: &mut SharedState) -> Result<()> {
        if FAV_UPDATED.swap(false, Ordering::SeqCst) {
            self.sync_local(s);
        }
        Ok(())
    }

    fn on_result(&mut self, res: Box<dyn Any>, s: &mut SharedState) -> Result<()> {
        let _res = match res.downcast::<bool>() {
            Err(res) => res,
            Ok(delete) => {
                self.tabs.selected_mut().view.on_result(s.t, *delete);
                return Ok(());
            }
        };
        Ok(())
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;
        if self.sync_fav_task.is_some() || self.export_task.is_some() || self.multi_create_fav_task.is_some() {
            return Ok(true);
        }
        let choose_cover = CHOOSE_COVER.load(Ordering::Relaxed);
        if !choose_cover {
            if self.order_menu.showing() {
                self.order_menu.touch(touch, t);
                return Ok(true);
            }
            if self.order_meta_menu.showing() {
                self.order_meta_menu.touch(touch, t);
                return Ok(true);
            }
            if self.multi_operation_menu.showing() {
                self.multi_operation_menu.touch(touch, t);
                return Ok(true);
            }
            if self.multi_select_menu.showing() {
                self.multi_select_menu.touch(touch, t);
                return Ok(true);
            }
            if self.tabs.touch(touch, s.rt) {
                return Ok(true);
            }
            if self.tags.touch(touch, t) {
                return Ok(true);
            }
            if self.rating.touch(touch, t) {
                return Ok(true);
            }
        }
        let charts_view = &mut self.tabs.selected_mut().view;
        if charts_view.transiting() {
            return Ok(true);
        }
        if charts_view.touch(touch, t, s.rt)? {
            return Ok(true);
        }
        if choose_cover {
            return Ok(true);
        }
        if !matches!(self.tabs.selected().ty, ChartListType::Local) {
            if self.prev_page_btn.touch(touch, t) {
                if self.current_page != 0 {
                    self.current_page -= 1;
                    self.load_online();
                }
                return Ok(true);
            }
            if self.next_page_btn.touch(touch, t) {
                if self.current_page + 1 < self.total_page() {
                    self.current_page += 1;
                    self.load_online();
                }
                return Ok(true);
            }
        }

        match self.tabs.selected().ty {
            ChartListType::Local => {
                if self.tabs.selected().view.multi_select.is_none() {
                    if self.import_btn.touch(touch, t) {
                        request_file("_import");
                        return Ok(true);
                    }
                    if self.fav_btn.touch(touch, t) {
                        self.next_page = Some(NextPage::Overlay(Box::new(FavoritesPage::new(
                            self.icons.clone(),
                            self.rank_icons.clone(),
                            self.current_fav_index,
                            None,
                        ))));
                        return Ok(true);
                    }
                }
                if !self.search_str.is_empty() && self.search_clr_btn.touch(touch) {
                    button_hit();
                    self.search_str.clear();
                    self.sync_local(s);
                    return Ok(true);
                }
                if !self.search_clr_btn.contains(touch.position) && self.search_btn.touch(touch, t) {
                    request_input("search", InputBox::new().default_text(&self.search_str));
                    return Ok(true);
                }
            }
            ChartListType::Ranked | ChartListType::Special | ChartListType::Unstable => {
                if !self.search_str.is_empty() && self.search_clr_btn.touch(touch) {
                    button_hit();
                    self.search_str.clear();
                    self.current_page = 0;
                    self.load_online();
                    return Ok(true);
                }
                if !self.search_clr_btn.contains(touch.position) && self.search_btn.touch(touch, t) {
                    request_input("search", InputBox::new().default_text(&self.search_str));
                    return Ok(true);
                }
                if self.filter_btn.touch(touch, t) {
                    if self.filter_show_tag {
                        self.tags.enter(t);
                    } else {
                        self.rating.enter(t);
                    }
                    return Ok(true);
                }
            }
            ChartListType::Popular => {}
        }
        if self.tabs.selected_mut().view.multi_select.is_some() {
            if self.multi_operation_btn.touch(touch, t) {
                let mut options = vec!["multi-export", "multi-create-fav"];
                if self.tabs.selected_mut().view.allow_edit {
                    options.push("multi-delete");
                }
                self.multi_operation_menu
                    .set_options(options.iter().map(|it| tl!(*it).into_owned()).collect());
                self.multi_operation_options = options;
                self.need_show_multi_operation_menu = true;
                return Ok(true);
            }
            if self.multi_select_btn.touch(touch, t) {
                self.need_show_multi_select_menu = true;
                return Ok(true);
            }
            if self.multi_select_cancel_btn.touch(touch, t) {
                self.tabs.selected_mut().view.multi_select = None;
                return Ok(true);
            }
        }
        if self.order_btn.touch(touch, t) {
            self.need_show_order_meta_menu = true;
            return Ok(true);
        }
        Ok(false)
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        let t = s.t;

        if let Some(chosen_cover) = CHOSEN_COVER.with(|it| it.borrow_mut().take()) {
            CHOOSE_COVER.store(false, Ordering::Relaxed);
            self.next_page = Some(NextPage::Overlay(Box::new(FavoritesPage::new(
                self.icons.clone(),
                self.rank_icons.clone(),
                self.current_fav_index,
                Some(chosen_cover),
            ))));
        }

        self.check_fav_page(s);

        if self.tabs.selected().ty == ChartListType::Local && self.current_order == ChartOrder::Rating {
            self.current_order = ChartOrder::Default;
            self.order_rev = true;
        }

        self.tags.update(t);
        self.rating.update(t);

        let is_local = self.tabs.selected().ty == ChartListType::Local;
        if self.tabs.changed() {
            self.tabs.selected_mut().view.reset_scroll();
            self.tabs.iter_mut().for_each(|it| it.view.multi_select = None);
            self.online_task = None;
            if is_local {
                self.sync_local(s);
            } else {
                self.current_page = 0;
                self.load_online();
            }
        }
        if self.tabs.selected_mut().view.clicked_special {
            let icons = Arc::clone(&self.icons);
            self.next_page_task = Some(Box::pin(async move { Ok(NextPage::Overlay(Box::new(CollectionPage::new(icons).await?))) }));
            self.tabs.selected_mut().view.clicked_special = false;
        }
        if let Some(task) = &mut self.next_page_task {
            if let Some(res) = poll_future(task.as_mut()) {
                self.next_page = Some(res?);
                self.next_page_task = None;
            }
        }

        if self.tags.show_rating {
            self.tags.show_rating = false;
            self.filter_show_tag = false;
            self.rating.enter(t);
        } else if self.tags_last_show && !self.tags.showing() {
            self.current_page = 0;
            self.load_online();
        }
        if self.rating.show_tags {
            self.rating.show_tags = false;
            self.filter_show_tag = true;
            self.tags.enter(t);
        } else if self.rating_last_show && !self.rating.showing() {
            self.current_page = 0;
            self.load_online();
        }
        self.tags_last_show = self.tags.showing();
        self.rating_last_show = self.rating.showing();
        if let Some(task) = &mut self.online_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => show_error(err.context(tl!("failed-to-load-online"))),
                    Ok(res) => {
                        self.online_total_page = res.2;
                        self.tabs.selected_mut().view.set(t, res.0);
                    }
                }
                self.online_task = None;
            }
        }
        self.order_menu.update(t);
        for chart in &mut s.charts_local {
            chart.illu.settle(t);
        }
        if self.tabs.selected_mut().view.update(t)? {
            self.load_online();
        }
        if self.tabs.selected_mut().view.need_update() {
            s.reload_local_charts();
            self.sync_local(s);
        }
        if let Some((id, text)) = take_input() {
            if id == "search" {
                self.search_str = text;
                if is_local {
                    self.sync_local(s);
                } else {
                    self.current_page = 0;
                    self.load_online();
                }
            } else if id == "new_fav" {
                if text.is_empty() {
                    use crate::page::favorites::{tl as ftl, L10N_LOCAL};
                    show_message(ftl!("name-empty")).error();
                } else {
                    let charts_view = &mut self.tabs.selected_mut().view;
                    if let Some(mut selected) = charts_view.multi_select.clone() {
                        self.multi_create_fav_task = Some(Task::new(async move {
                            let mut ids_str = String::new();
                            for chart in &selected {
                                if let ChartRef::Online(id, None) = chart {
                                    ids_str.push_str(&id.to_string());
                                    ids_str.push(',');
                                }
                            }
                            if !ids_str.is_empty() {
                                ids_str.pop();
                                let resp: Vec<Chart> = recv_raw(Client::get(format!("/chart/multi-get?ids={ids_str}"))).await?.json().await?;
                                let mut id_to_chart = HashMap::new();
                                for chart in resp {
                                    id_to_chart.insert(chart.id, Box::new(chart));
                                }
                                for chart in &mut selected {
                                    if let ChartRef::Online(id, chart_info) = chart {
                                        if chart_info.is_none() {
                                            *chart_info = Some(id_to_chart.get(id).cloned().unwrap());
                                        }
                                    }
                                }
                            }
                            Ok(CreateFavorite {
                                name: text,
                                charts: selected,
                            })
                        }));
                    }
                }
            } else {
                return_input(id, text);
            }
        }
        if let Some(task) = &mut self.multi_create_fav_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => show_error(err),
                    Ok(result) => {
                        self.tabs.selected_mut().view.multi_select = None;
                        let data = get_data_mut();
                        let mut col = LocalCollection::new(result.name);
                        col.charts = result.charts;
                        data.push_collection(col)?;
                        let _ = save_data();
                        show_message(tl!("fav-created")).ok();
                        self.current_fav_index = Some(data.collection_uuids().len() - 1);
                        self.sync_local(s);
                    }
                }
                self.multi_create_fav_task = None;
            }
        }
        if self.delete_multi.swap(false, Ordering::Relaxed) {
            let selected = self.tabs.selected_mut().view.multi_select.take().unwrap();
            let selected = selected.into_iter().collect::<HashSet<_>>();
            let data = get_data_mut();
            let mut local_paths = HashSet::new();
            for chart in &selected {
                let path = chart.local_path();
                match std::fs::remove_dir_all(format!("{}/{path}", dir::charts()?)) {
                    Ok(_) => {}
                    Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                    Err(err) => return Err(err.into()),
                }
                local_paths.insert(path);
            }
            data.charts.retain(|it| !local_paths.contains(it.local_path.as_str()));
            let _ = save_data();
            show_message(tl!("multi-deleted")).ok();
            s.reload_local_charts();
            self.sync_local(s);
        }
        if self.order_meta_menu.changed() {
            match self.order_meta_menu.selected() {
                0 => {
                    self.need_show_order_menu = true;
                }
                1 => {
                    self.order_rev = !self.order_rev;
                    self.update_order_meta_menu_options();
                    self.order_meta_menu.set_selected(usize::MAX);
                    self.on_order_update(s);
                }
                _ => {}
            }
        }
        if self.order_menu.changed() {
            self.current_order = self.order_menu_options[self.order_menu.selected()];
            self.order_rev = self.current_order == ChartOrder::Default;
            self.order_meta_menu.set_selected(usize::MAX);
            self.update_order_meta_menu_options();
            self.on_order_update(s);
        }
        if self.multi_operation_menu.changed() {
            let charts_view = &mut self.tabs.selected_mut().view;
            let selected = charts_view.multi_select.as_mut().unwrap();
            match self.multi_operation_options[self.multi_operation_menu.selected()] {
                "multi-export" => {
                    let charts = dir::charts()?;
                    let mut paths = Vec::with_capacity(selected.len());
                    let mut non_existent = Vec::new();
                    for chart in selected {
                        let path: PathBuf = format!("{charts}/{}", chart.local_path()).into();
                        if !path.exists() {
                            let mut charts = charts_view.charts.as_ref().unwrap().iter().filter_map(|it| it.chart.as_ref());
                            non_existent.push(charts.find(|it| &it.to_ref() == chart).unwrap().info.name.clone());
                        } else {
                            paths.push(chart.local_path().into_owned());
                        }
                    }
                    if !non_existent.is_empty() {
                        Dialog::simple(tl!("multi-export-no-file", "charts" => non_existent.join(", "))).show();
                    } else {
                        self.export_paths = Some(paths);
                        request_export();
                    }
                }
                "multi-create-fav" => {
                    request_input("new_fav", InputBox::new());
                }
                "multi-delete" => {
                    confirm_dialog(ttl!("del-confirm"), tl!("multi-delete-confirm", "count" => selected.len()), self.delete_multi.clone());
                }
                _ => {}
            }
        }
        if self.multi_select_menu.changed() {
            let charts_view = &mut self.tabs.selected_mut().view;
            let sel = charts_view.multi_select.as_mut().unwrap();
            let charts = charts_view.charts.as_ref().unwrap();
            match self.multi_select_menu.selected() {
                0 => {
                    sel.clear();
                    sel.extend(charts.iter().filter_map(|it| it.chart.as_ref()).map(ChartItem::to_ref));
                }
                1 => {
                    let old_sel = mem::take(sel).into_iter().collect::<HashSet<_>>();
                    for chart in charts {
                        if let Some(chart) = &chart.chart {
                            let r = chart.to_ref();
                            if !old_sel.contains(&r) {
                                sel.push(r);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        self.multi_operation_menu.update(t);
        if JUST_LOADED_TOS.fetch_and(false, Ordering::Relaxed) {
            check_read_tos_and_policy(false, false);
        }
        let list = self.tabs.selected_mut();
        let view = &mut list.view;
        if let Some((from, to)) = view.take_movement() {
            if self.current_order != ChartOrder::Default && self.current_fav_index.is_none() {
                show_message(tl!("order-update-failed-sort")).error();
                return Ok(());
            }
            let data = get_data_mut();
            if let Some(index) = self.current_fav_index {
                let uuid = data.collection_uuids()[index];
                let mut col = data.collection_info(&uuid).as_ref().clone();
                let online = col.id.is_some();
                let chart = col.charts.remove(from);
                col.charts.insert(to, chart);
                data.set_collection_info(&uuid, col)?;
                let _ = save_data();
                if online && !data.config.offline_mode {
                    if let Some(task) = FavoritesPage::sync_to_cloud_task(index, false) {
                        self.sync_fav_task = Some(task);
                    }
                }
            } else {
                if self.order_rev {
                    let chart = data.charts.remove(data.charts.len() - from - 1);
                    data.charts.insert(data.charts.len() - to, chart);
                } else {
                    let chart = data.charts.remove(from);
                    data.charts.insert(to, chart);
                }
                let _ = save_data();
                s.reload_local_charts();
            }
            show_message(tl!("order-updated")).ok();
        }
        view.allow_edit(
            list.ty == ChartListType::Local
                && self.search_str.is_empty()
                && self.current_fav_index.is_none_or(|it| get_data().collection_by_index(it).is_owned()),
        );

        if let Some(task) = &mut self.sync_fav_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => show_error(err.context(tl!("fav-sync-failed"))),
                    Ok(Some(col)) => {
                        let data = get_data();
                        let uuid = data.collection_uuids()[self.current_fav_index.unwrap()];
                        let local = data.collection_info(&uuid);
                        data.set_collection_info(&uuid, local.merge(&col))?;
                        let _ = save_data();
                        show_message(tl!("fav-synced")).ok();
                    }
                    Ok(None) => {
                        use crate::page::favorites::{tl as ftl, L10N_LOCAL};
                        confirm_dialog(ftl!("sync-to-cloud"), ftl!("sync-outdated"), self.force_sync_to_cloud.clone());
                    }
                }
                self.sync_fav_task = None;
            }
        }
        if self.force_sync_to_cloud.swap(false, Ordering::SeqCst) {
            if let Some(index) = self.current_fav_index {
                if let Some(task) = FavoritesPage::sync_to_cloud_task(index, true) {
                    self.sync_fav_task = Some(task);
                }
            }
        }
        if let Some(config) = EXPORT_CONFIG.lock().unwrap().take() {
            fn export_inner(paths: Vec<String>, output: File, progress: Arc<AtomicU32>) -> Result<()> {
                let charts = dir::charts()?;
                let mut zip = zip::ZipWriter::new(BufWriter::new(output));
                let options = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored)
                    .unix_permissions(0o755);
                for (i, name) in paths.iter().enumerate() {
                    zip.start_file(format!("{name}.zip"), options)?;
                    let chart_bytes = compress_folder(Path::new(&format!("{charts}/{name}")))?;
                    zip.write_all(&chart_bytes)?;
                    progress.store(i as u32 + 1, Ordering::Relaxed);
                }

                zip.start_file("export.json", options.compression_method(zip::CompressionMethod::Deflated))?;
                let info = ExportInfo {
                    exported_at: Utc::now(),
                    version: env!("CARGO_PKG_VERSION").to_owned(),
                };
                serde_json::to_writer(&mut zip, &info)?;

                zip.finish()?;
                Ok(())
            }

            match config {
                Err(err) => show_error(err.into()),
                Ok(config) => {
                    if let Some(paths) = self.export_paths.take() {
                        self.export_total = paths.len();
                        let (tx, rx) = mpsc::sync_channel(1);
                        let progress = self.export_progress.clone();
                        progress.store(0, Ordering::SeqCst);
                        std::thread::spawn(move || {
                            let result = export_inner(paths, config.file, progress);
                            if result.is_err() {
                                if let Err(err) = (config.deleter)() {
                                    warn!("failed to delete export file: {:?}", err);
                                }
                            }
                            let _ = tx.send(result);
                        });
                        self.export_task = Some(rx);
                    }
                }
            }
        }
        if let Some(rx) = &mut self.export_task {
            match rx.try_recv() {
                Ok(Err(err)) => {
                    show_error(err);
                    self.export_task = None;
                }
                Ok(Ok(())) => {
                    #[cfg(target_os = "ios")]
                    {
                        if let Some(path) = EXPORT_PICKER_PATH.lock().unwrap().clone() {
                            present_export_picker(path);
                        } else {
                            show_message(tl!("multi-exported")).ok();
                        }
                    }
                    #[cfg(not(target_os = "ios"))]
                    show_message(tl!("multi-exported")).ok();
                    self.tabs.selected_mut().view.multi_select = None;
                    self.export_task = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    show_error(Error::msg("Export thread panicked"));
                    self.export_task = None;
                }
            }
        }

        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        self.check_fav_page(s);

        let t = s.t;
        let rt = s.rt;
        let mut r = ui.content_rect();
        let chosen = self.tabs.selected().ty;
        if chosen != ChartListType::Local {
            r.h -= 0.08;
        }
        s.render_fader(ui, |ui| {
            self.tabs.render(ui, rt, r, |ui, list| {
                list.view.render(ui, r.feather(-0.01), t);
                Ok(())
            })
        })?;
        if chosen != ChartListType::Popular {
            s.render_fader(ui, |ui| {
                let multi_select = self.tabs.selected().view.multi_select.is_some();
                let mut r = Rect::new(r.right(), -ui.top + 0.04, 0., r.y + ui.top - 0.06);
                r.w = r.h;
                r.x -= r.w;

                // 多选模式操作按钮
                if let Some(selected) = &mut self.tabs.selected_mut().view.multi_select {
                    self.multi_operation_btn.render_shadow(ui, r, t, |ui, path| {
                        ui.fill_path(&path, WHITE);
                        let cr = r.feather(-0.01);
                        ui.fill_rect(cr, (*self.icons.r#mod, cr, ScaleType::Fit, BLACK));
                    });
                    if self.need_show_multi_operation_menu {
                        self.need_show_multi_operation_menu = false;
                        self.multi_operation_menu
                            .set_auto_adjust(Some(ui.screen_rect().nonuniform_feather(-0.03, -0.05)));
                        self.multi_operation_menu.set_bottom(true);
                        self.multi_operation_menu.set_selected(usize::MAX);
                        self.multi_operation_menu.show(ui, t, Rect::new(r.x, r.bottom() + 0.02, 0.35, 0.3));
                    }

                    let text = tl!("multi-select-status", "count" => selected.len());
                    let tw = ui.text(&text).size(0.5).measure().w;
                    let w = tw + 0.1;
                    let sr = Rect::new(r.x - w - 0.02, r.y, w, r.h);
                    self.multi_select_btn.render_shadow(ui, sr, t, |ui, path| {
                        ui.fill_path(&path, WHITE);
                        let ir = Rect::new(sr.x + 0.04, sr.center().y, 0., 0.).feather(0.025);
                        ui.fill_rect(ir, (*self.icons.select, ir, ScaleType::Fit, BLACK));
                        ui.text(text)
                            .pos((ir.right() + sr.right() - 0.01) / 2., sr.center().y)
                            .size(0.5)
                            .anchor(0.5, 0.5)
                            .no_baseline()
                            .color(BLACK)
                            .draw();
                    });
                    if self.need_show_multi_select_menu {
                        self.need_show_multi_select_menu = false;
                        self.multi_select_menu
                            .set_auto_adjust(Some(ui.screen_rect().nonuniform_feather(-0.03, -0.05)));
                        self.multi_select_menu.set_bottom(true);
                        self.multi_select_menu.set_selected(usize::MAX);
                        self.multi_select_menu.show(ui, t, Rect::new(r.x, r.bottom() + 0.02, 0.3, 0.2));
                    }
                    r.x = sr.x - r.w - 0.02;

                    self.multi_select_cancel_btn.render_shadow(ui, r, t, |ui, path| {
                        ui.fill_path(&path, WHITE);
                        let cr = r.feather(-0.01);
                        ui.fill_rect(cr, (*self.icons.close, cr, ScaleType::Fit, BLACK));
                    });
                    r.x -= r.w + 0.02;
                }

                if chosen == ChartListType::Local && !multi_select {
                    self.import_btn.render_shadow(ui, r, t, |ui, path| {
                        ui.fill_path(&path, semi_black(0.4));
                        let cr = r.feather(-0.01);
                        ui.fill_rect(cr, (*self.icons.plus, cr, ScaleType::Fit));
                    });
                    r.x -= r.w + 0.02;
                }

                if chosen != ChartListType::Local {
                    self.filter_btn.render_shadow(ui, r, t, |ui, path| {
                        ui.fill_path(&path, semi_black(0.4));
                        let cr = r.feather(-0.01);
                        ui.fill_rect(cr, (*self.icons.filter, cr, ScaleType::Fit));
                    });
                    r.x -= r.w + 0.02;
                } else if !multi_select {
                    let active = self.current_fav_index.is_some();
                    self.fav_btn.render_shadow(ui, r, t, |ui, path| {
                        ui.fill_path(&path, if active { WHITE } else { semi_black(0.4) });
                        let cr = r.feather(-0.01);
                        if active {
                            ui.fill_rect(cr, (*self.icons.star, cr, ScaleType::Fit, Color::from_rgba(255, 193, 7, 255)));
                        } else {
                            ui.fill_rect(cr, (*self.icons.star_outline, cr, ScaleType::Fit));
                        }
                    });
                    r.x -= r.w + 0.02;
                }

                self.order_btn.render_shadow(ui, r, t, |ui, path| {
                    ui.fill_path(&path, semi_black(0.4));
                    let cr = r.feather(-0.01);
                    ui.fill_rect(cr, (*self.icons.order, cr, ScaleType::Fit));
                });
                if self.need_show_order_meta_menu {
                    self.need_show_order_meta_menu = false;
                    self.order_meta_menu
                        .set_auto_adjust(Some(ui.screen_rect().nonuniform_feather(-0.03, -0.05)));
                    if self.tabs.selected().ty == ChartListType::Local {
                        self.order_menu_options = vec![ChartOrder::Default, ChartOrder::Name, ChartOrder::Difficulty];
                    } else {
                        self.order_menu_options = vec![ChartOrder::Default, ChartOrder::Rating, ChartOrder::Name, ChartOrder::Difficulty];
                    }
                    self.order_meta_menu.set_bottom(true);
                    self.order_meta_menu.set_auto_dismiss(false);
                    self.update_order_meta_menu_options();
                    self.order_meta_menu.set_selected(usize::MAX);
                    self.order_meta_menu.show(ui, t, Rect::new(r.x, r.bottom() + 0.02, 0.35, 0.2));
                }

                let empty = self.search_str.is_empty();
                r.w = 0.53;
                r.x -= r.w + 0.02;
                if empty {
                    r.x += r.h;
                    r.w -= r.h;
                }
                let rt = r.right();
                self.search_btn.render_shadow(ui, r, t, |ui, path| {
                    ui.fill_path(&path, semi_black(0.4));
                });
                let mut r = r.feather(-0.01);
                r.w = r.h;
                if !empty {
                    ui.fill_rect(r, (*self.icons.close, r, ScaleType::Fit));
                    self.search_clr_btn.set(ui, r);
                    r.x += r.w;
                }
                ui.fill_rect(r, (*self.icons.search, r, ScaleType::Fit));
                ui.text(&self.search_str)
                    .pos(r.right() + 0.01, r.center().y)
                    .anchor(0., 0.5)
                    .no_baseline()
                    .size(0.6)
                    .max_width(rt - r.right() - 0.02)
                    .draw();
            });
        }
        if chosen != ChartListType::Local {
            let total_page = self.total_page();
            s.render_fader(ui, |ui| {
                let cx = r.center().x;
                let r = ui
                    .text(tl!("page", "current" => self.current_page + 1, "total" => total_page))
                    .pos(cx, r.bottom() + 0.034)
                    .anchor(0.5, 0.)
                    .no_baseline()
                    .size(0.5)
                    .draw();
                let dist = 0.3;
                let ft = 0.024;
                let prev_page = tl!("prev-page");
                let r = ui.text(prev_page.deref()).pos(cx - dist, r.y).anchor(0.5, 0.).size(0.5).measure();
                self.prev_page_btn.render_text(ui, r.feather(ft), t, prev_page, 0.5, false);
                let next_page = tl!("next-page");
                let r = ui.text(next_page.deref()).pos(cx + dist, r.y).anchor(0.5, 0.).size(0.5).measure();
                self.next_page_btn.render_text(ui, r.feather(ft), t, next_page, 0.5, false);
            });
        }
        self.order_menu.render(ui, t, 1.);
        self.order_meta_menu.render(ui, t, 1.);
        if self.need_show_order_menu {
            self.need_show_order_menu = false;
            self.order_menu.set_bottom(true);
            self.order_menu.set_selected(
                self.order_menu_options
                    .iter()
                    .position(|&it| it == self.current_order)
                    .unwrap_or(usize::MAX),
            );
            self.order_menu
                .set_options(self.order_menu_options.iter().map(|it| it.label().into_owned()).collect());

            let mut r = self.order_meta_menu.rect();
            r.w = 0.3;
            r.x -= r.w + 0.02;
            r.h = 0.4;
            self.order_menu.show(ui, t, r);
        }
        self.multi_select_menu.render(ui, t, 1.);
        self.multi_operation_menu.render(ui, t, 1.);
        self.tags.render(ui, t);
        self.rating.render(ui, t);
        Ok(())
    }

    fn render_top(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        self.tabs.selected_mut().view.render_top(ui, t);
        if self.sync_fav_task.is_some() {
            ui.full_loading_simple(t);
        }
        if self.export_task.is_some() {
            let current = self.export_progress.load(Ordering::Relaxed);
            let total = self.export_total;
            ui.full_loading(tl!("multi-exporting", "current" => current, "total" => total), t);
        }
        if self.multi_create_fav_task.is_some() {
            ui.full_loading_simple(t);
        }
        Ok(())
    }

    fn next_page(&mut self) -> NextPage {
        self.next_page.take().unwrap_or_default()
    }

    fn next_scene(&mut self, _s: &mut SharedState) -> NextScene {
        self.tabs.selected_mut().view.next_scene().unwrap_or_default()
    }
}
