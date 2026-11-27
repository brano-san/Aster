use std::ffi::c_void;
use std::ptr::null_mut;
use windows::core::{IUnknown, Interface, RuntimeName, BOOL, GUID, HRESULT};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::*;

macro_rules! nullable_interface {
    ($interface:ident, $vtable:ident, $sdk:ident, $sdk_vtable:ident, $methods:ident,
        $($method:ident($($arg:ident: $ty:ty),*)),+ $(,)?) => {
        #[derive(Clone)]
        #[repr(transparent)]
        pub struct $interface(IUnknown);

        unsafe impl Interface for $interface {
            type Vtable = $vtable;
            const IID: GUID = $sdk::IID;
        }

        windows_core::imp::interface_hierarchy!($interface, windows_core::IUnknown);
        impl RuntimeName for $interface {}

        #[repr(transparent)]
        pub struct $vtable($sdk_vtable);

        impl $vtable {
            pub fn matches(iid: &GUID) -> bool {
                $sdk_vtable::matches(iid)
            }

            pub const fn new<Identity: $methods, const OFFSET: isize>() -> Self {
                $(
                    #[allow(non_snake_case)]
                    unsafe extern "system" fn $method<Identity: $methods, const OFFSET: isize>(
                        this: *mut c_void,
                        $($arg: $ty,)*
                        result: *mut *mut c_void,
                    ) -> HRESULT {
                        unsafe {
                            result.write(null_mut());
                            ($sdk_vtable::new::<Identity, OFFSET>().$method)(this, $($arg,)* result)
                        }
                    }
                )+
                Self($sdk_vtable {
                    $($method: $method::<Identity, OFFSET>,)+
                    ..$sdk_vtable::new::<Identity, OFFSET>()
                })
            }
        }
    };
}

nullable_interface!(
    Simple, SimpleVtbl, IRawElementProviderSimple, IRawElementProviderSimple_Vtbl, IRawElementProviderSimple_Impl,
    GetPatternProvider(pattern: UIA_PATTERN_ID), HostRawElementProvider(),
);
nullable_interface!(
    Fragment, FragmentVtbl, IRawElementProviderFragment, IRawElementProviderFragment_Vtbl, IRawElementProviderFragment_Impl,
    Navigate(direction: NavigateDirection),
);
nullable_interface!(
    FragmentRoot, FragmentRootVtbl, IRawElementProviderFragmentRoot, IRawElementProviderFragmentRoot_Vtbl, IRawElementProviderFragmentRoot_Impl,
    ElementProviderFromPoint(x: f64, y: f64), GetFocus(),
);
nullable_interface!(
    TextRange, TextRangeVtbl, ITextRangeProvider, ITextRangeProvider_Vtbl, ITextRangeProvider_Impl,
    FindAttribute(attribute: UIA_TEXTATTRIBUTE_ID, value: VARIANT, backward: BOOL),
    FindText(text: *mut c_void, backward: BOOL, ignore_case: BOOL),
);
