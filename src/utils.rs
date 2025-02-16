pub mod utils {
    use encoding_rs::GBK;
    use std::ffi::{c_char, CStr, CString};
    use std::marker::PhantomData;
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::sync::mpsc::{Receiver, Sender};
    use std::sync::{mpsc, Arc, Mutex};
    use std::{fs, ptr, thread};

    /// 定义一个对外的 C 接口，执行外部命令
    /// 该接口使用原始指针和长度来传递命令字符串，以适应 C 语言的调用习惯
    #[repr(C)]
    pub struct CommandResult {
        pub success: bool,
        pub stdout: *mut c_char,
        pub stderr: *mut c_char,
    }
    impl CommandResult {

        fn new(success: bool, stdout: *mut c_char, stderr: *mut c_char) -> Self {
            CommandResult { success, stdout, stderr }
        }

        // 提供一个方法来安全地释放由 CommandResult 包含的 C 字符串

        pub fn free(&self) {
            unsafe {
                if !self.stdout.is_null() {
                    let _ = CString::from_raw(self.stdout);
                }
                if !self.stderr.is_null() {
                    let _ = CString::from_raw(self.stderr);
                }
            }
        }
    }

    /// 将 C 风格字符串转换为 Rust `String`。
    ///
    /// # 参数
    ///
    /// * `s` - 指向 C 风格字符串的指针 (`*const c_char`)。该字符串应以空字符结尾。
    ///
    /// # 返回值
    ///
    /// * `Ok(String)` - 如果转换成功，返回包含转换后字符串的 `Result::Ok`。
    /// * `Err(std::str::Utf8Error)` - 如果输入的 C 字符串包含无效的 UTF-8 序列，则返回 `Result::Err` 包含一个 `std::str::Utf8Error`。
    ///
    /// # 安全性
    ///
    /// 该函数使用了 `unsafe` 块来进行裸指针操作。调用者必须确保传入的指针是有效的，并且指向一个以空字符结尾的 C 风格字符串。如果指针为空，函数将安全地返回一个空字符串。


    pub fn cstring_to_string(s: *const c_char) -> String {
        unsafe {
            if s.is_null() {
                return String::new();
            }
            let c_str = CStr::from_ptr(s);
            // 使用 to_string_lossy 确保总是返回一个有效的 String
            c_str.to_string_lossy().into_owned()
        }
    }

    /// 释放 `CommandResult` 结构体中包含的 C 字符串内存
    #[no_mangle]
    pub extern "C" fn free_command_result(result: CommandResult) {
        result.free();
    }

    /// 执行外部命令并返回结果
    ///
    /// # 参数
    ///
    /// * `command` - 指向 C 风格字符串的指针 (`*const c_char`)，表示要执行的命令。
    ///
    /// # 返回值
    ///
    /// 返回一个 `CommandResult` 结构体，包含命令执行的结果。
    fn exec<T: AsRef<str>>(command: T) -> CommandResult {
        let com_str = command.as_ref();

        #[cfg(target_os = "windows")]
        let shell = "cmd";
        #[cfg(not(target_os = "windows"))]
        let shell = "sh";

        #[cfg(target_os = "windows")]
        let arg = "/C";
        #[cfg(not(target_os = "windows"))]
        let arg = "-c";

        #[cfg(target_os = "windows")]
        let all_com = format!("chcp 65001 > nul && {}", com_str);
        #[cfg(not(target_os = "windows"))]
        let all_com = com_str.to_string();

        let output = Command::new(shell)
            .arg(arg)
            .arg(&all_com)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .expect("Failed to execute command");

        // 处理输出
        let (decoded_stdout, _, had_errors) = GBK.decode(&output.stdout);
        let stdout = if had_errors {
            println!("Decoding issues encountered for stdout.");
            String::from_utf8_lossy(&output.stdout).into_owned()
        } else {
            decoded_stdout.into_owned()
        };

        let (decoded_stderr, _, had_errors) = GBK.decode(&output.stderr);
        let stderr = if had_errors {
            println!("Decoding issues encountered for stderr.");
            String::from_utf8_lossy(&output.stderr).into_owned()
        } else {
            decoded_stderr.into_owned()
        };

        CommandResult::new(
            output.status.success(),
            str_to_cstr(stdout),
            str_to_cstr(stderr),
        )
    }

    // 外部 C 接口
    #[no_mangle]
    pub extern "C" fn C_exec(command: *const c_char) -> CommandResult {
        // 将 C 字符串转换为 Rust 字符串
        let com_str = unsafe {
            match CStr::from_ptr(command).to_str() {
                Ok(s) => s,
                Err(_) => return CommandResult::new(false, ptr::null_mut(), ptr::null_mut()),
            }
        };

        exec(com_str)
    }

    /// 释放 `CString` 内存的函数
    ///
    /// 这个函数是为了提供给 C 语言代码使用的，因此使用 `extern "C"` 声明。
    ///
    /// # 参数
    ///
    /// * `ptr` - 一个指向 C 字符串的指针。
    #[no_mangle]
    pub extern "C" fn free_cstring(ptr: *mut c_char) {
        // 使用 `unsafe` 块，因为涉及到直接操作原始指针
        unsafe {
            // 检查指针是否为空，避免传入无效指针导致的错误
            if ptr.is_null() {
                return;
            }
            // 通过 `from_raw` 方法将指针转换回 `CString`，这会自动释放内存
            // 这里使用 `_` 来忽略掉 `CString` 实例，因为我们只关心内存释放
            let _ = CString::from_raw(ptr);
        }
    }

    /// 释放并重置 C 字符串指针
    ///
    /// 该函数旨在与 C 代码互操作，通过释放动态分配的 C 字符串并将其指针设置为 `NULL` 来避免内存泄漏。
    /// 它使用 `CString::from_raw` 从原始指针获取所有权并安全地释放内存，然后重置指针。
    ///
    /// # 参数
    ///
    /// * `ptr` - 一个指向 C 字符串的指针引用，该字符串将被释放并重置。
    ///
    /// # 安全性
    ///
    /// 此函数涉及不安全代码块，因为它处理原始指针。必须确保在释放内存后指针不会再次被使用，以避免悬挂指针。
    /// 通过将指针设置为 `NULL`，我们确保了这一点。
    #[no_mangle]
    pub extern "C" fn free_and_reset_c_string(ptr: &mut *const c_char) {
        unsafe {
            if !ptr.is_null() {
                // 从原始指针获取所有权并释放内存
                let _ = CString::from_raw(*ptr as *mut _);
                // 重置指针为 `NULL`，避免悬挂指针
                *ptr = ptr::null();
            }
        }
    }

    /// 将 Rust 字符串转换为 C 风格的字符串
    ///
    /// 此函数接收一个 Rust `String` 类型的参数，并将其转换为 `*const c_char` 类型，
    /// 即 C 语言中字符串的指针类型。这一转换是为了在 Rust 代码中调用 C 语言库函数时，
    /// 能够传递字符串参数给 C 函数。
    ///
    /// # 参数
    ///
    /// * `s` - 一个 `String` 类型的参数，代表需要转换的 Rust 字符串。
    ///
    /// # 返回值
    ///
    /// 返回一个 `*const c_char` 类型的指针，指向转换后的 C 风格字符串。
    ///
    /// # 安全性
    ///
    /// 调用此函数的代码需要确保在使用完指针后正确地释放内存，以避免内存泄漏。
    /// 此外，由于返回的是一个原始指针，使用时需要确保不会造成未定义行为，例如
    /// 解引用悬挂指针等。
    pub fn str_to_cstr<T: AsRef<str>>(s: T) -> *mut c_char {
        // 尝试将输入转换为 CString
        match CString::new(s.as_ref()) {
            Ok(c_string) => c_string.into_raw(), // 转换成功，返回原始指针
            Err(_) => ptr::null_mut(), // 如果包含 NUL 字符，返回空指针
        }
    }
    /// 检查指定路径的文件是否存在
    ///
    /// # Parameters
    ///
    /// * `file_path` - 文件路径的C字符串指针
    ///
    /// # Returns
    ///
    /// * `1` - 文件存在
    /// * `0` - 文件不存在
    /// * `-1` - 发生其他错误
    #[no_mangle]
    pub extern "C" fn check_file(file_path: *const c_char) -> i32 {
        // 将 C 字符串转换为 Rust 字符串
        let file_path_str = unsafe {
            match CStr::from_ptr(file_path).to_str() {
                Ok(s) => s,
                Err(_) => return -1, // 如果转换失败，返回-1
            }
        };

        check_file_generic(file_path_str)
    }

    // 泛型函数，支持多种字符串类型
    fn check_file_generic<T: AsRef<Path>>(file_path: T) -> i32 {
        match fs::metadata(file_path) {
            Ok(_) => 1, // 文件存在，返回1
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    0 // 文件不存在，返回0
                } else {
                    -1 // 其他错误发生，返回-1
                }
            }
        }
    }




    /// 将字符串按行分割成向量
    ///
    /// 该函数接受一个字符串切片作为输入，并将其按照行分隔符分割成一个字符串向量
    /// 主要用于处理需要按行显示或处理的文本数据
    ///
    /// # 参数
    ///
    /// * `text`: &str - 需要被分割的字符串切片
    ///
    /// # 返回值
    ///
    /// 返回一个`Vec<String>`，其中每个元素都是原字符串中的一行
    ///

    pub fn split_by_newline(text: &str) -> Vec<String> {
        // 使用lines方法按行分隔字符串，然后使用map方法将每一行转换为String类型，最后收集到一个Vec中
        text.lines().map(String::from).collect()
    }
    type Job<R> = Box<dyn FnOnce() -> R + Send>;

    enum Message<R> {
        NewJob(Job<R>, Sender<R>),
        Terminate,
    }

    struct Worker<T, R> {
        id: usize,
        thread: Option<thread::JoinHandle<()>>,
        _phantom: PhantomData<(T, R)>,
    }

    impl<T, R> Worker<T, R>
    where
        T: Send + 'static,
        R: Send + 'static,
    {
        fn new(id: usize, receiver: Arc<Mutex<Receiver<Message<R>>>>) -> Worker<T, R> {
            let thread = thread::spawn(move || loop {
                let message = receiver.lock().unwrap().recv().unwrap();

                match message {
                    Message::NewJob(job, tx) => {
                        let result = job();
                        tx.send(result).expect("Failed to send result");
                    },
                    Message::Terminate => break,
                }
            });

            Worker {
                id,
                thread: Some(thread),
                _phantom: PhantomData,
            }
        }
    }

    pub struct ThreadPool<T, R> {
        workers: Vec<Worker<T, R>>,
        sender: Sender<Message<R>>,
    }

    impl<T, R> ThreadPool<T, R>
    where
        T: Send + 'static,
        R: Send + 'static,
    {
        pub fn new(size: usize) -> ThreadPool<T, R> {
            assert!(size > 0);

            let (sender, receiver) = mpsc::channel();
            let receiver = Arc::new(Mutex::new(receiver));
            let mut workers = Vec::with_capacity(size);

            for id in 0..size {
                workers.push(Worker::new(id, Arc::clone(&receiver)));
            }

            ThreadPool { workers, sender }
        }

        pub fn submit<F>(&self, task: F, arg: T) -> Receiver<R>
        where
            F: FnOnce(T) -> R + Send + 'static,
        {
            let (tx, rx) = mpsc::channel();
            // 创建一个新的闭包，该闭包捕获了 `task` 和 `arg`
            let job = Box::new(move || task(arg));
            self.sender.send(Message::NewJob(job, tx)).unwrap();
            rx
        }
    }

    impl<T, R> Drop for ThreadPool<T, R> {
        fn drop(&mut self) {
            for _ in &mut self.workers {
                self.sender.send(Message::Terminate).unwrap();
            }

            for worker in &mut self.workers {
                if let Some(thread) = worker.thread.take() {
                    thread.join().unwrap();
                }
            }
        }
    }
    #[no_mangle]
    pub extern "C" fn C_utf_8_str_to_gbk_str(utf8_str: *const c_char) -> *mut c_char {
        // 将 C 字符串转换为 Rust 字符串
        let input_str = unsafe { CStr::from_ptr(utf8_str).to_string_lossy().into_owned() };

        // 进行编码转换
        let (encoded_bytes, _, had_errors) = GBK.encode(&input_str);

        if had_errors {
            println!("Warning: encountered errors during encoding.");
        }

        // 将 GBK 编码的字节数组转换为 C 字符串
        match CString::new(encoded_bytes.into_owned()) {
            Ok(c_string) => c_string.into_raw(), // 返回 C 字符串指针
            Err(_) => ptr::null_mut(), // 如果转换失败，返回空指针
        }
    }
    pub fn utf_8_str_to_gbk_str(utf8_str: &str)-> String {
        // 进行编码转换
        let (encoded_bytes, _, had_errors) = GBK.encode(&utf8_str);

        if had_errors {
            println!("Warning: encountered errors during encoding.")
        }
        String::from_utf8_lossy(&encoded_bytes).into_owned()
    }
}