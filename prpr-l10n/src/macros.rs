#[macro_export]
macro_rules! langs {
    ($($lang_id:literal: $lang_name:expr),*) => {
        macro_rules! __create_bundles_builder {
            ($d:tt) => {
                #[macro_export]
                macro_rules! create_bundles {
                    ($d file:literal) => {{
                        let mut bundles = Vec::new();
                        $(
                            bundles.push($crate::create_bundle!($lang_id, $file));
                        )*
                        bundles
                    }};
                }
            }
        }

        macro_rules! __count_builder {
            ($d:tt) => {
                macro_rules! count {
                    () => (0usize);
                    ($d _a:tt $d _b:tt $d _c:tt $d _d:tt $d _e:tt $d ($d tail:tt)*) => (
                        5usize + count!($d ($d tail)*)
                    );
                    ($d _a:tt $d _b:tt $d ($d tail:tt)*) => (2usize + count!($d ($d tail)*));
                    ($d _a:tt $d ($d tail:tt)*) => (1usize + count!($d ($d tail)*));
                }
            }
        }

        __create_bundles_builder!($);

        __count_builder!($);

        pub const LANG_COUNT: usize = count!($($lang_id)*);

        pub static LANGS: [&str; LANG_COUNT] = [$($lang_id,)*];
        pub static LANG_NAMES: [&str; LANG_COUNT] = [$($lang_name,)*];
        pub static LANG_IDENTS: Lazy<[LanguageIdentifier; LANG_COUNT]> = Lazy::new(|| LANGS.map(|lang| lang.parse().unwrap()));
    };
}

#[macro_export]
macro_rules! create_bundle {
    ($locale:literal, $file:literal) => {{
        let mut bundle = $crate::FluentBundle::new($crate::LANG_IDENTS.iter().cloned().collect());
        bundle
            .add_resource(
                $crate::FluentResource::try_new(
                    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/locales/", $locale, "/", $file, ".ftl")).to_owned(),
                )
                .unwrap(),
            )
            .unwrap();
        bundle.set_use_isolating(false);
        bundle
    }};
}

#[macro_export]
macro_rules! tl_file {
    ($file:literal) => {
        $crate::tl_file!($file tl);
    };
    ($file:literal $macro_name:ident $($p:tt)*) => {
        static L10N_BUNDLES: $crate::Lazy<$crate::L10nBundles> = $crate::Lazy::new(|| $crate::create_bundles!($file).into());

        thread_local! {
            pub static L10N_LOCAL: std::cell::RefCell<$crate::L10nLocal> = $crate::L10nLocal::new(&*L10N_BUNDLES).into();
        }

        macro_rules! __tl_builder {
            ($d:tt) => {
                macro_rules! $macro_name {
                    ($d key:expr) => {
                        $($p)* L10N_LOCAL.with(|it| it.borrow_mut().format($key, None))
                    };
                    ($d key:expr, $d args:expr) => {
                        $($p)* L10N_LOCAL.with(|it| it.borrow_mut().format($key, Some($args)))
                    };
                    ($d key:expr, $d ($d name:expr => $d value:expr),+) => {
                        $($p)* L10N_LOCAL.with(|it| it.borrow_mut().format($key, Some(&$crate::fluent_args![$d($d name => $d value), *])).to_string())
                    };
                    (err $d ($d body:tt)*) => {
                        anyhow::Error::msg($macro_name!($d($d body)*))
                    };
                    (bail $d ($d body:tt)*) => {
                        return anyhow::Result::Err(anyhow::Error::msg($macro_name!($d($d body)*)))
                    };
                }

                pub(crate) use $macro_name;
            }
        }

        __tl_builder!($);
    };
}
