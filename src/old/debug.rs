thread_local! {
    pub(crate) static DBG_INDENT: ::std::cell::Cell<usize> = ::std::cell::Cell::new(0);
}
pub(crate) const INDENT: usize = 2;

macro_rules! enter_function {
    () => {
        crate::debug::DBG_INDENT.with(|x| x.set(x.get() + crate::debug::INDENT));
    };
}

macro_rules! exit_function {
    () => {
        crate::debug::DBG_INDENT.with(|x| x.set(x.get() - crate::debug::INDENT));
    };
}

pub(crate) const COLUMN_1: usize = 25usize;

macro_rules! dbg_file_line {
    () => {
        print!(
            "{0:<1$}{2:>3$}",
            format!("{}:{}:", file!(), line!()),
            crate::debug::COLUMN_1,
            "",
            crate::debug::DBG_INDENT.with(|x| x.get())
        );
    };
}

macro_rules! dbg_println {
    ($fmt:literal) => {
        dbg_file_line!();
        println!($fmt);
    };
    ($fmt:literal, $($args:expr),+) => {
        dbg_file_line!();
        println!($fmt, $($args),+);
    };
}

macro_rules! dbg_call {
    ($fmt:literal) => {{
        dbg_println!("fn {} {{", $fmt);
        enter_function!();
    }};
    ($fmt:literal, $($args:expr),+) => {{

        dbg_println!("fn {} {{", format!($fmt, $($args),+));
        enter_function!();
    }}
}

macro_rules! dbg_return {
    () => {
        exit_function!();
        dbg_println!("}} => ()");
    };
    ($fmt:literal) => {
        exit_function!();
        dbg_println!("}} => {}", $fmt);
    };
    ($fmt:literal, $val:expr) => {{
        let res = $val;
        exit_function!();
        dbg_println!("}} => {}", format!($fmt, res));
        res
    }};
    ($fmt:literal, $val:expr, $($args:expr),*) => {{
        let res = $val;
        exit_function!();
        dbg_println!("}} => {}", format!($fmt, res, $($args),*));
        res
    }};
}

macro_rules! dbg {
    ($val:expr) => {{
        let res = $val;
        dbg_println!("{:?}", res);
        res
    }};
    ($fmt:literal, $val:expr) => {{
        let res = $val;
        dbg_println!($fmt, res);
        res
    }};
    ($fmt:literal, $val:expr, $($args:expr),*) => {{
        let res = $val;
        dbg_println!($fmt, res, $($args),*);
        res
    }};
}
