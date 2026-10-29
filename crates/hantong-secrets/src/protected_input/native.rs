use super::*;
use std::cell::Cell;
use windows_sys::Win32::{
    Foundation::HWND,
    Graphics::Gdi::{GetObjectType, OBJ_FONT},
    Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom},
    System::Threading::{GetCurrentProcessId, GetCurrentThreadId},
    UI::{
        Controls::{EM_EMPTYUNDOBUFFER, EM_GETLIMITTEXT, EM_SETLIMITTEXT},
        Input::KeyboardAndMouse::{EnableWindow, GetFocus, IsWindowEnabled, SetFocus},
        Shell::{DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};

const SUBCLASS_ID: usize = 0x4d48_5054;

// The wrapper and installed callback each retain a strong reference. If an OS
// destroy fails, the callback's reference survives the wrapper until NCDESTROY.
// No freed wrapper allocation is used as subclass reference data.
struct Lifetime {
    alive: Cell<bool>,
    hook_reference: Cell<bool>,
}
pub(super) struct NativeInput {
    hwnd: HWND,
    parent: HWND,
    root_owner: HWND,
    thread: u32,
    id: u16,
    lifetime: Rc<Lifetime>,
}

unsafe extern "system" fn subclass(
    hwnd: HWND,
    message: u32,
    wparam: usize,
    lparam: isize,
    _id: usize,
    data: usize,
) -> isize {
    // The installed strong reference is released exactly once at NCDESTROY.
    // An additional reference covers synchronous reentrancy through DefSubclassProc.
    let pointer = data as *const Lifetime;
    if pointer.is_null() {
        return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
    }
    let lifetime = unsafe {
        Rc::increment_strong_count(pointer);
        Rc::from_raw(pointer)
    };
    if message == WM_NCDESTROY {
        lifetime.alive.set(false);
        unsafe { RemoveWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID) };
        if lifetime.hook_reference.replace(false) {
            unsafe { Rc::decrement_strong_count(pointer) };
        }
    }
    if matches!(message, WM_COPY | WM_CUT | WM_CONTEXTMENU) {
        0
    } else {
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }
}

pub(super) fn random_nonce() -> Result<[u8; 32], SecretError> {
    let mut value = [0; 32];
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            value.as_mut_ptr(),
            32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status < 0 || value == [0; 32] {
        Err(SecretError::Protection)
    } else {
        Ok(value)
    }
}

fn owned_thread(hwnd: HWND, thread: u32) -> bool {
    let mut process = 0;
    let actual = unsafe { GetWindowThreadProcessId(hwnd, &mut process) };
    actual != 0 && actual == thread && process == unsafe { GetCurrentProcessId() }
}

fn root_owner(parent: HWND, thread: u32) -> Result<HWND, SecretError> {
    if parent.is_null()
        || !owned_thread(parent, thread)
        || unsafe { IsWindow(parent) } == 0
        || unsafe { GetWindowLongPtrW(parent, GWL_STYLE) } as u32 & WS_CHILD != 0
        || unsafe { GetAncestor(parent, GA_ROOT) } != parent
    {
        return Err(SecretError::InvalidInputWindow);
    }
    let owner = unsafe { GetAncestor(parent, GA_ROOTOWNER) };
    if owner.is_null() || !owned_thread(owner, thread) {
        return Err(SecretError::InvalidInputWindow);
    }
    Ok(owner)
}

pub(super) fn attach(
    plan: ImapPasswordCapturePlan,
    parent: isize,
    id: u16,
) -> Result<PasswordInput, SecretError> {
    if plan.started.elapsed() >= INPUT_TTL {
        return Err(SecretError::InputExpired);
    }
    let parent = parent as HWND;
    let thread = unsafe { GetCurrentThreadId() };
    let owner = root_owner(parent, thread)?;
    if id == 0 || !unsafe { GetDlgItem(parent, i32::from(id)) }.is_null() {
        return Err(SecretError::InvalidInputWindow);
    }
    let class = [69_u16, 68, 73, 84, 0];
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            std::ptr::null(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_PASSWORD as u32 | ES_AUTOHSCROLL as u32,
            0,
            0,
            1,
            1,
            parent,
            usize::from(id) as _,
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        return Err(SecretError::InvalidInputWindow);
    }
    // No plaintext exists before this guard/subclass is installed.
    let lifetime = Rc::new(Lifetime {
        alive: Cell::new(true),
        hook_reference: Cell::new(true),
    });
    let pointer = Rc::into_raw(Rc::clone(&lifetime));
    if unsafe { SetWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID, pointer as usize) } == 0 {
        unsafe {
            drop(Rc::from_raw(pointer));
            DestroyWindow(hwnd);
        }
        return Err(SecretError::InvalidInputWindow);
    }
    let input = PasswordInput {
        native: NativeInput {
            hwnd,
            parent,
            root_owner: owner,
            thread,
            id,
            lifetime,
        },
        plan: Some(plan),
        _thread_bound: PhantomData,
    };
    unsafe { SendMessageW(hwnd, EM_SETLIMITTEXT, MAX_PASSWORD_BYTES, 0) };
    input.native.check()?;
    Ok(input)
}

impl NativeInput {
    fn identity(&self) -> Result<(), SecretError> {
        // Check the thread/liveness before making any call with a stale HWND.
        if !self.lifetime.alive.get() || self.thread != unsafe { GetCurrentThreadId() } {
            return Err(SecretError::InvalidInputWindow);
        }
        let mut reference = 0;
        if !owned_thread(self.hwnd, self.thread)
            || unsafe { GetWindowSubclass(self.hwnd, Some(subclass), SUBCLASS_ID, &mut reference) }
                == 0
            || reference != Rc::as_ptr(&self.lifetime) as usize
        {
            return Err(SecretError::InvalidInputWindow);
        }
        Ok(())
    }

    pub(super) fn check(&self) -> Result<(), SecretError> {
        self.identity()?;
        if root_owner(self.parent, self.thread)? != self.root_owner
            || unsafe { GetParent(self.hwnd) } != self.parent
            || unsafe { GetAncestor(self.hwnd, GA_ROOT) } != self.parent
            || unsafe { GetAncestor(self.hwnd, GA_ROOTOWNER) } != self.root_owner
            || unsafe { GetDlgCtrlID(self.hwnd) } != i32::from(self.id)
            || unsafe { IsWindowUnicode(self.hwnd) } == 0
        {
            return Err(SecretError::InvalidInputWindow);
        }
        let style = unsafe { GetWindowLongPtrW(self.hwnd, GWL_STYLE) } as u32;
        let mut class = [0_u16; 8];
        let length = unsafe { GetClassNameW(self.hwnd, class.as_mut_ptr(), class.len() as i32) };
        let is_edit = length == 4
            && class[..4]
                .iter()
                .zip(b"EDIT")
                .all(|(a, b)| *a <= 127 && (*a as u8).eq_ignore_ascii_case(b));
        let required =
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_PASSWORD as u32 | ES_AUTOHSCROLL as u32;
        if !is_edit
            || style & required != required
            || style & ES_MULTILINE as u32 != 0
            || unsafe { SendMessageW(self.hwnd, EM_GETLIMITTEXT, 0, 0) }
                != MAX_PASSWORD_BYTES as isize
        {
            return Err(SecretError::InvalidInputWindow);
        }
        Ok(())
    }

    pub(super) fn set_bounds(
        &self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> Result<(), SecretError> {
        self.check()?;
        if width <= 0 || height <= 0 {
            return Err(SecretError::InvalidInputWindow);
        }
        if unsafe { MoveWindow(self.hwnd, x, y, width, height, 0) } == 0 {
            return Err(SecretError::InvalidInputWindow);
        }
        self.check()
    }
    pub(super) fn focus(&self) -> Result<(), SecretError> {
        self.check()?;
        // SetFocus returns the previous focus (which can legitimately be NULL).
        unsafe { SetFocus(self.hwnd) };
        self.check()?;
        if unsafe { GetFocus() } == self.hwnd {
            Ok(())
        } else {
            Err(SecretError::InvalidInputWindow)
        }
    }
    pub(super) fn set_enabled(&self, enabled: bool) -> Result<(), SecretError> {
        self.check()?;
        // EnableWindow reports the old disabled state, not success/failure.
        unsafe { EnableWindow(self.hwnd, i32::from(enabled)) };
        self.check()?;
        if (unsafe { IsWindowEnabled(self.hwnd) } != 0) == enabled {
            Ok(())
        } else {
            Err(SecretError::InvalidInputWindow)
        }
    }
    pub(super) fn set_font(&self, font: isize) -> Result<(), SecretError> {
        self.check()?;
        if font == 0 || unsafe { GetObjectType(font as _) } != OBJ_FONT as u32 {
            return Err(SecretError::InvalidInputWindow);
        }
        // No synchronous repaint while the parent's mutable state may be borrowed.
        unsafe { SendMessageW(self.hwnd, WM_SETFONT, font as usize, 0) };
        self.check()
    }
    pub(super) fn matches(&self, candidate: isize) -> bool {
        self.hwnd as isize == candidate && self.check().is_ok()
    }

    pub(super) fn destroy(&mut self) -> Result<(), SecretError> {
        if !self.lifetime.alive.get() {
            return Ok(());
        }
        // Cleanup needs exact ownership, not the original layout/style: a failed
        // reparent/style validation must still destroy our own credential child.
        self.identity()?;
        let cleared = unsafe { SetWindowTextW(self.hwnd, [0_u16].as_ptr()) } != 0;
        unsafe { SendMessageW(self.hwnd, EM_EMPTYUNDOBUFFER, 0, 0) };
        let empty = unsafe { SendMessageW(self.hwnd, WM_GETTEXTLENGTH, 0, 0) } == 0;
        let destroyed = unsafe { DestroyWindow(self.hwnd) } != 0;
        if !cleared || !empty || !destroyed || self.lifetime.alive.get() {
            return Err(SecretError::InvalidInputWindow);
        }
        Ok(())
    }

    pub(super) fn capture(&self) -> Result<SensitiveBuffer, SecretError> {
        self.check()?;
        let length = unsafe { SendMessageW(self.hwnd, WM_GETTEXTLENGTH, 0, 0) };
        if length < 0 {
            return Err(SecretError::InvalidInputWindow);
        }
        if !(1..=MAX_PASSWORD_BYTES as isize).contains(&length) {
            return Err(SecretError::InvalidCredential);
        }
        let mut wide = SensitiveWide(vec![0; length as usize + 1]);
        let copied = unsafe {
            SendMessageW(
                self.hwnd,
                WM_GETTEXT,
                wide.0.len(),
                wide.0.as_mut_ptr() as isize,
            )
        };
        self.check()?;
        if copied != length
            || wide.0[length as usize] != 0
            || unsafe { SendMessageW(self.hwnd, WM_GETTEXTLENGTH, 0, 0) } != length
        {
            return Err(SecretError::InvalidInputWindow);
        }
        decode_password(&wide.0[..length as usize])
    }
}
impl Drop for NativeInput {
    fn drop(&mut self) {
        // If this fails the installed callback still owns its state; it can
        // safely observe a later parent destruction. Never free a live callback.
        let _ = self.destroy();
    }
}

struct SensitiveWide(Vec<u16>);
impl Drop for SensitiveWide {
    fn drop(&mut self) {
        for unit in &mut self.0 {
            unsafe { std::ptr::write_volatile(unit, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}
fn decode_password(wide: &[u16]) -> Result<SensitiveBuffer, SecretError> {
    // Allocate the whole output before writing a single plaintext byte. All
    // error paths wipe it; no extend/reallocation or unwiped String is involved.
    let mut bytes = SensitiveBuffer(vec![0; MAX_PASSWORD_BYTES]);
    let mut used = 0;
    for result in char::decode_utf16(wide.iter().copied()) {
        let character = result.map_err(|_| SecretError::InvalidCredential)?;
        if character.is_control() {
            return Err(SecretError::InvalidCredential);
        }
        let mut scratch = [0; 4];
        let count = character.encode_utf8(&mut scratch).len();
        if count > MAX_PASSWORD_BYTES - used {
            crate::wipe(&mut scratch);
            return Err(SecretError::InvalidCredential);
        }
        bytes.0[used..used + count].copy_from_slice(&scratch[..count]);
        crate::wipe(&mut scratch);
        used += count;
    }
    bytes.0.truncate(used); // the unused tail has always remained zero
    validate_password(bytes.as_slice())?;
    Ok(bytes)
}

#[cfg(test)]
mod tests;
