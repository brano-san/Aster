use windows::core::{implement, Interface, Result, BOOL, GUID, HSTRING, PCWSTR};
use windows::Foundation::{IPropertyValue, PropertyValue};
use windows::Graphics::Effects::{IGraphicsEffect, IGraphicsEffect_Impl, IGraphicsEffectSource, IGraphicsEffectSource_Impl};
use windows::System::{DispatcherQueue, DispatcherQueueController};
use windows::UI::Composition::{CompositionEffectBrush, CompositionEffectSourceParameter, Compositor, Visual};
use windows::UI::Composition::Core::CompositorController;
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::Win32::Foundation::{E_INVALIDARG, E_NOTIMPL, HWND};
use windows::Win32::Graphics::Direct2D::{
    CLSID_D2D1GaussianBlur, D2D1_GAUSSIANBLUR_OPTIMIZATION_BALANCED, D2D1_GAUSSIANBLUR_PROP_BORDER_MODE,
    D2D1_GAUSSIANBLUR_PROP_OPTIMIZATION, D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION,
};
use windows::Win32::Graphics::Direct2D::Common::D2D1_BORDER_MODE_HARD;
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_HOSTBACKDROPBRUSH};
use windows::Win32::System::WinRT::{
    CreateDispatcherQueueController, DispatcherQueueOptions, RoInitialize, RoUninitialize, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT,
    RO_INIT_SINGLETHREADED,
};
use windows::Win32::System::WinRT::Composition::ICompositorDesktopInterop;
use windows::Win32::System::WinRT::Graphics::Direct2D::{
    IGraphicsEffectD2D1Interop, IGraphicsEffectD2D1Interop_Impl, GRAPHICS_EFFECT_PROPERTY_MAPPING, GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT,
};
use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MsgWaitForMultipleObjectsEx, PeekMessageW, TranslateMessage, MSG, MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, WM_QUIT};
use windows_collections::IIterable;
use windows_future::AsyncStatus;
use windows_numerics::Vector2;

const BLUR_AMOUNT: &str = "Blur.BlurAmount";
const SOURCE: &str = "background";
const MAX_STANDARD_DEVIATION: f32 = 40.0;

struct RuntimeApartment;

impl Drop for RuntimeApartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

struct Dispatcher(DispatcherQueueController);

impl Drop for Dispatcher {
    fn drop(&mut self) {
        let Ok(shutdown) = self.0.ShutdownQueueAsync() else { return };
        // The controller belongs to the UI thread and completes shutdown via its message queue.
        unsafe {
            let mut message = MSG::default();
            while matches!(shutdown.Status(), Ok(AsyncStatus::Started)) {
                if PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    if message.message != WM_QUIT {
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                } else {
                    let _ = MsgWaitForMultipleObjectsEx(None, 100, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
                }
            }
        }
    }
}

pub(crate) struct BlurHost {
    compositor: Compositor,
    controller: CompositorController,
    _dispatcher: Option<Dispatcher>,
    _apartment: RuntimeApartment,
}

impl BlurHost {
    pub(crate) fn new() -> Result<Self> {
        unsafe { RoInitialize(RO_INIT_SINGLETHREADED)? };
        let apartment = RuntimeApartment;
        let dispatcher = if DispatcherQueue::GetForCurrentThread().is_ok() {
            None
        } else {
            let options = DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_NONE,
            };
            Some(Dispatcher(unsafe { CreateDispatcherQueueController(options)? }))
        };
        let controller = CompositorController::new()?;
        let compositor = controller.Compositor()?;
        Ok(Self { compositor, controller, _dispatcher: dispatcher, _apartment: apartment })
    }
}

pub(crate) struct BackgroundBlur {
    target: DesktopWindowTarget,
    brush: CompositionEffectBrush,
    controller: CompositorController,
    _host_backdrop: HostBackdrop,
}

impl BackgroundBlur {
    pub(crate) fn new(host: &BlurHost, hwnd: HWND, amount: f32) -> Result<Self> {
        set_host_backdrop(hwnd, true)?;
        let host_backdrop = HostBackdrop(hwnd);
        let compositor = &host.compositor;
        let interop: ICompositorDesktopInterop = compositor.cast()?;
        let target = unsafe { interop.CreateDesktopWindowTarget(hwnd, false) }?;

        let source = HSTRING::from(SOURCE);
        let parameter = CompositionEffectSourceParameter::Create(&source)?;
        let effect: IGraphicsEffect = GaussianBlur { source: parameter.cast()? }.into();
        let properties = IIterable::<HSTRING>::from(vec![HSTRING::from(BLUR_AMOUNT)]);
        let brush = compositor.CreateEffectFactoryWithProperties(&effect, &properties)?.CreateBrush()?;
        brush.SetSourceParameter(&source, &compositor.CreateHostBackdropBrush()?)?;

        let visual = compositor.CreateSpriteVisual()?;
        visual.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
        visual.SetBrush(&brush)?;
        let blur = Self { target, brush, controller: host.controller.clone(), _host_backdrop: host_backdrop };
        blur.target.SetRoot(&visual)?;
        blur.set_amount(amount)?;
        Ok(blur)
    }

    pub(crate) fn set_amount(&self, amount: f32) -> Result<()> {
        self.brush.Properties()?.InsertScalar(&HSTRING::from(BLUR_AMOUNT), amount * MAX_STANDARD_DEVIATION)?;
        self.controller.Commit()
    }
}

impl Drop for BackgroundBlur {
    fn drop(&mut self) {
        let _ = self.target.SetRoot(None::<&Visual>);
        let _ = self.controller.Commit();
    }
}

struct HostBackdrop(HWND);

impl Drop for HostBackdrop {
    fn drop(&mut self) {
        let _ = set_host_backdrop(self.0, false);
    }
}

fn set_host_backdrop(hwnd: HWND, enabled: bool) -> Result<()> {
    let enabled = BOOL::from(enabled);
    unsafe { DwmSetWindowAttribute(hwnd, DWMWA_USE_HOSTBACKDROPBRUSH, &enabled as *const BOOL as _, size_of::<BOOL>() as u32) }
}

// The composition effect factory reads the Direct2D Gaussian blur description from this object.
#[implement(IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectD2D1Interop)]
struct GaussianBlur {
    source: IGraphicsEffectSource,
}

impl IGraphicsEffectSource_Impl for GaussianBlur_Impl {}

impl IGraphicsEffect_Impl for GaussianBlur_Impl {
    fn Name(&self) -> Result<HSTRING> {
        Ok(HSTRING::from("Blur"))
    }

    fn SetName(&self, _: &HSTRING) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
}

impl IGraphicsEffectD2D1Interop_Impl for GaussianBlur_Impl {
    fn GetEffectId(&self) -> Result<GUID> {
        Ok(CLSID_D2D1GaussianBlur)
    }

    fn GetNamedPropertyMapping(&self, name: &PCWSTR, index: *mut u32, mapping: *mut GRAPHICS_EFFECT_PROPERTY_MAPPING) -> Result<()> {
        if name.is_null() || index.is_null() || mapping.is_null() {
            return Err(E_INVALIDARG.into());
        }
        let name = unsafe { name.to_string() }.map_err(|_| E_INVALIDARG)?;
        if name != "BlurAmount" {
            return Err(E_INVALIDARG.into());
        }
        unsafe {
            index.write(D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION.0 as u32);
            mapping.write(GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT);
        }
        Ok(())
    }

    fn GetPropertyCount(&self) -> Result<u32> {
        Ok(3)
    }

    fn GetProperty(&self, index: u32) -> Result<IPropertyValue> {
        match index as i32 {
            index if index == D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION.0 => PropertyValue::CreateSingle(0.0),
            index if index == D2D1_GAUSSIANBLUR_PROP_OPTIMIZATION.0 => PropertyValue::CreateUInt32(D2D1_GAUSSIANBLUR_OPTIMIZATION_BALANCED.0 as u32),
            index if index == D2D1_GAUSSIANBLUR_PROP_BORDER_MODE.0 => PropertyValue::CreateUInt32(D2D1_BORDER_MODE_HARD.0 as u32),
            _ => return Err(E_INVALIDARG.into()),
        }?.cast()
    }

    fn GetSource(&self, index: u32) -> Result<IGraphicsEffectSource> {
        if index == 0 { Ok(self.source.clone()) } else { Err(E_INVALIDARG.into()) }
    }

    fn GetSourceCount(&self) -> Result<u32> {
        Ok(1)
    }
}
