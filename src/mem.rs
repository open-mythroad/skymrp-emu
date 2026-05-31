pub type GuestUSize = u32;

#[repr(transparent)]
pub struct Ptr<T, const MUT: bool>(GuestUSize, std::marker::PhantomData<T>);

impl<T, const MUT: bool> Clone for Ptr<T, MUT> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T, const MUT: bool> Copy for Ptr<T, MUT> {}

/// Constant guest pointer type (like Rust's `*const T`).
pub type ConstPtr<T> = Ptr<T, false>;
/// Mutable guest pointer type (like Rust's `*mut T`).
pub type MutPtr<T> = Ptr<T, true>;

impl<T, const MUT: bool> Ptr<T, MUT> {
    pub fn to_bits(self) -> GuestUSize {
        self.0
    }
    pub fn from_bits(bits: GuestUSize) -> Self {
        Ptr(bits, std::marker::PhantomData)
    }

    pub fn cast<U>(self) -> Ptr<U, MUT> {
        Ptr::<U, MUT>::from_bits(self.to_bits())
    }
}

pub trait SafeRead {}
impl SafeRead for i8 {}
impl SafeRead for u8 {}
impl SafeRead for i16 {}
impl SafeRead for u16 {}
impl SafeRead for i32 {}
impl SafeRead for u32 {}
impl SafeRead for i64 {}
impl SafeRead for u64 {}
impl SafeRead for f32 {}
impl SafeRead for f64 {}
impl<T, const MUT: bool> SafeRead for Ptr<T, MUT> {}

type Bytes = [u8; 1 << 32];

pub struct Memory {
    bytes: *mut Bytes,
}

impl Drop for Memory {
    fn drop(&mut self) {
        let layout = std::alloc::Layout::new::<Bytes>();
        unsafe {
            std::alloc::dealloc(self.bytes as *mut _, layout);
        }
    }
}

impl Memory {
    pub fn new() -> Memory {
        let layout = std::alloc::Layout::new::<Bytes>();
        let bytes = unsafe { std::alloc::alloc_zeroed(layout) as *mut Bytes };

        Memory { bytes }
    }

    fn bytes(&self) -> &Bytes {
        unsafe { &*self.bytes }
    }
    fn bytes_mut(&mut self) -> &mut Bytes {
        unsafe { &mut *self.bytes }
    }

    pub fn bytes_at<const MUT: bool>(&self, ptr: Ptr<u8, MUT>, count: GuestUSize) -> &[u8] {
        &self.bytes()[ptr.to_bits() as usize..][..count as usize]
    }
    pub fn bytes_at_mut(&mut self, ptr: MutPtr<u8>, count: GuestUSize) -> &mut [u8] {
        &mut self.bytes_mut()[ptr.to_bits() as usize..][..count as usize]
    }

    pub fn read<T, const MUT: bool>(&self, ptr: Ptr<T, MUT>) -> T
    where
        T: SafeRead,
    {
        let size = std::mem::size_of::<T>().try_into().unwrap();
        let slice = self.bytes_at(ptr.cast(), size);
        let ptr: *const T = slice.as_ptr().cast();
        unsafe { ptr.read_unaligned() }
    }
    pub fn write<T>(&mut self, ptr: MutPtr<T>, value: T) {
        let size = std::mem::size_of::<T>().try_into().unwrap();
        let slice = self.bytes_at_mut(ptr.cast(), size);
        let ptr: *mut T = slice.as_mut_ptr().cast();
        unsafe { ptr.write_unaligned(value) }
    }
}
