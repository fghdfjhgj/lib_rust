pub mod utils;
pub use utils::utils as other_utils;
pub mod web;
pub mod flash_phone;
pub mod ai;
pub use ai::ai as other_ai ;
pub mod sql;
mod kernel;
pub use kernel::kernel as other_kernel ;
pub use flash_phone::flash_phone as other_flash_phone ;
pub use sql::sql as other_sql ;
pub use web::web as other_web ;
#[cfg(test)]
mod tests {

    use crate::utils::utils::*;
    #[test]
    fn it_works() {

        let c=str_to_cstr("echo 你好".parse().unwrap());
        let result = exec(c).stdout;
        println!("{}",cstring_to_string(result));
    }
}