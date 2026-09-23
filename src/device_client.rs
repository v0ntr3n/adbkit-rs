//! Commands for a single device.
//!
//! Ported from `src/adb/DeviceClient.ts`.

use tokio::io::AsyncRead;

use crate::client::Client;
use crate::command::{self, Arg};
use crate::connection::Connection;
use crate::error::{Error, Result};
use crate::jdwptracker::JdwpTracker;
use crate::linetransform::LineTransform;
use crate::logcat::Reader as LogcatReader;
use crate::proc::stat::ProcStat;
use crate::sync::{Entry, PullOptions, PullTransfer, PushOptions, Stats, Sync};
use crate::types::{
    Extras, Features, Forward, Properties, Reverse, StartActivityOptions, StartServiceOptions,
};
use crate::ShellStream;

/// A screenshot stream.
///
/// Usually a PNG straight from the device's `screencap` utility, but on older
/// devices it falls back to the framebuffer converted with GraphicsMagick,
/// which is why the concrete type is erased.
pub type ScreenshotStream = std::pin::Pin<Box<dyn tokio::io::AsyncRead + Send>>;

/// The source of a file to push to the device.
pub enum PushSource {
    /// A path on the local file system.
    File(String),
    /// Anything that can be read asynchronously.
    Stream(Box<dyn AsyncRead + Unpin + Send>),
}

impl PushSource {
    /// A path on the local file system.
    pub fn file(path: impl Into<String>) -> Self {
        PushSource::File(path.into())
    }
}

impl<T: AsyncRead + Unpin + Send + 'static> From<T> for PushSource {
    fn from(stream: T) -> Self {
        PushSource::Stream(Box::new(stream))
    }
}

/// A client for a single device, selected by serial number.
///
/// ```no_run
/// # async fn example() -> Result<(), adbkit::Error> {
/// use adbkit::Adb;
///
/// let client = Adb::create_client(Default::default());
/// let device = client.get_device("ZX1G22K9QM");
/// let props = device.get_properties().await?;
/// println!("The model is {}", props.get("ro.product.model").unwrap());
/// # Ok(())
/// # }
/// ```
pub struct DeviceClient {
    client: Client,
    serial: String,
}

impl DeviceClient {
    /// Creates a client for the device with the given serial number.
    pub fn new(client: Client, serial: impl Into<String>) -> Self {
        DeviceClient {
            client,
            serial: serial.into(),
        }
    }

    /// The client this device client was created from.
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// The serial number of the device.
    pub fn serial(&self) -> &str {
        &self.serial
    }

    async fn connection(&self) -> Result<Connection<tokio::net::TcpStream>> {
        self.client.connection().await
    }

    /// Opens a new connection and selects this device on it.
    ///
    /// The connection can then be used for further device commands. This is the
    /// equivalent of `adb -s <serial> shell`, i.e. of the TypeScript
    /// `DeviceClient.transport()`.
    pub async fn transport(&self) -> Result<Connection<tokio::net::TcpStream>> {
        let mut connection = self.connection().await?;
        command::host::transport(&mut connection, &self.serial).await?;
        Ok(connection)
    }

    /// Gets the serial number of the device. With our API this doesn't really
    /// make much sense, but it has been implemented for completeness.
    pub async fn get_serial_no(&self) -> Result<String> {
        let mut connection = self.connection().await?;
        command::host_serial::get_serial_no(&mut connection, &self.serial).await
    }

    /// Gets the device path of the device.
    pub async fn get_device_path(&self) -> Result<String> {
        let mut connection = self.connection().await?;
        command::host_serial::get_device_path(&mut connection, &self.serial).await
    }

    /// Gets the state of the device, e.g. `device` or `offline`.
    pub async fn get_state(&self) -> Result<String> {
        let mut connection = self.connection().await?;
        command::host_serial::get_state(&mut connection, &self.serial).await
    }

    /// Retrieves the properties of the device. Analogous to
    /// `adb shell getprop`.
    pub async fn get_properties(&self) -> Result<Properties> {
        let mut transport = self.transport().await?;
        command::host_transport::get_properties(&mut transport).await
    }

    /// Retrieves the features of the device. Analogous to
    /// `adb shell pm list features`.
    pub async fn get_features(&self) -> Result<Features> {
        let mut transport = self.transport().await?;
        command::host_transport::get_features(&mut transport).await
    }

    /// Retrieves the list of packages present on the device. Analogous to
    /// `adb shell pm list packages`.
    pub async fn get_packages(&self, flags: Option<&str>) -> Result<Vec<String>> {
        let mut transport = self.transport().await?;
        command::host_transport::get_packages(&mut transport, flags).await
    }

    /// Attempts to retrieve the IP address of the device. Roughly analogous to
    /// `adb shell getprop dhcp.<iface>.ipaddress`.
    pub async fn get_dhcp_ip_address(&self, iface: Option<&str>) -> Result<String> {
        let iface = iface.unwrap_or("wlan0");
        let properties = self.get_properties().await?;
        match properties.get(&format!("dhcp.{}.ipaddress", iface)) {
            Some(ip) => Ok(ip.clone()),
            None => Err(Error::Device(format!(
                "Unable to find ipaddress for '{}'",
                iface
            ))),
        }
    }

    /// Forwards socket connections from the ADB server host (local) to the
    /// device (remote). Analogous to `adb forward <local> <remote>`.
    pub async fn forward(&self, local: &str, remote: &str) -> Result<bool> {
        let mut connection = self.connection().await?;
        command::host_serial::forward(&mut connection, &self.serial, local, remote).await
    }

    /// Lists forwarded connections on the device. Analogous to
    /// `adb forward --list`.
    pub async fn list_forwards(&self) -> Result<Vec<Forward>> {
        let mut connection = self.connection().await?;
        command::host_serial::list_forwards(&mut connection, &self.serial).await
    }

    /// Reverses socket connections from the device (remote) to the ADB server
    /// host (local). Analogous to `adb reverse <remote> <local>`.
    pub async fn reverse(&self, remote: &str, local: &str) -> Result<bool> {
        let mut transport = self.transport().await?;
        command::host_transport::reverse(&mut transport, remote, local).await
    }

    /// Lists reversed connections on the device. Analogous to
    /// `adb reverse --list`.
    pub async fn list_reverses(&self) -> Result<Vec<Reverse>> {
        let mut transport = self.transport().await?;
        command::host_transport::list_reverses(&mut transport).await
    }

    /// Runs a shell command on the device. Note that you'll be limited to the
    /// permissions of the `shell` user, which ADB uses.
    pub async fn shell(
        &self,
        command: &str,
    ) -> Result<crate::connection::RawStream<tokio::net::TcpStream>> {
        let transport = self.transport().await?;
        command::host_transport::shell(transport, command).await
    }

    /// Runs a shell command, escaping each argument. When an array is given,
    /// the elements are rudimentarily escaped (for convenience, not security)
    /// and joined to form a command.
    pub async fn shell_args(
        &self,
        args: &[Arg],
    ) -> Result<crate::connection::RawStream<tokio::net::TcpStream>> {
        let command = command::host_transport::shell_command(args);
        self.shell(&command).await
    }

    /// Reboots the device.
    pub async fn reboot(&self) -> Result<bool> {
        let mut transport = self.transport().await?;
        command::host_transport::reboot(&mut transport).await
    }

    /// Attempts to remount the `/system` partition in read-write mode.
    pub async fn remount(&self) -> Result<bool> {
        let mut transport = self.transport().await?;
        command::host_transport::remount(&mut transport).await
    }

    /// Puts the device into root mode.
    pub async fn root(&self) -> Result<bool> {
        let mut transport = self.transport().await?;
        command::host_transport::root(&mut transport).await
    }

    /// Starts a JDWP tracker for the device.
    ///
    /// Note that as the tracker keeps a connection open, you must drop it (or
    /// call [`JdwpTracker::end`]) if you wish to stop tracking JDWP processes.
    pub async fn track_jdwp(&self) -> Result<JdwpTracker> {
        let transport = self.transport().await?;
        command::host_transport::track_jdwp(transport).await
    }

    /// Fetches the current **raw** framebuffer (i.e. what is visible on the
    /// screen) from the device, and optionally converts it into something more
    /// usable by using GraphicsMagick's `gm` command.
    pub async fn framebuffer(
        &self,
        format: Option<&str>,
    ) -> Result<crate::framebuffer::FramebufferStream<tokio::net::TcpStream>> {
        let format = format.unwrap_or("raw");
        let transport = self.transport().await?;
        command::host_transport::framebuffer(transport, format).await
    }

    /// Takes a screenshot in PNG format using the built-in `screencap`
    /// utility.
    ///
    /// If the `screencap` command fails (e.g. because it doesn't exist on
    /// older devices), we fall back to [`DeviceClient::framebuffer`] in PNG
    /// mode, which is slower and has additional installation requirements.
    pub async fn screencap(&self) -> Result<ScreenshotStream> {
        let transport = self.transport().await?;
        match command::host_transport::screencap(transport).await {
            Ok(stream) => Ok(Box::pin(stream)),
            Err(err) => {
                crate::debug!("Emulating screencap command due to '{}'", err);
                let transport = self.transport().await?;
                let stream = command::host_transport::framebuffer(transport, "png").await?;
                Ok(Box::pin(stream))
            }
        }
    }

    /// Opens a direct connection to a unix domain socket in the given path.
    pub async fn open_local(&self, path: &str) -> Result<ShellStream> {
        let transport = self.transport().await?;
        command::host_transport::open_local(transport, path).await
    }

    /// Opens a direct connection to a binary log file, providing access to the
    /// raw log data.
    pub async fn open_log(&self, name: &str) -> Result<ShellStream> {
        let transport = self.transport().await?;
        command::host_transport::open_log(transport, name).await
    }

    /// Opens a direct TCP connection to a port on the device, without any port
    /// forwarding required.
    pub async fn open_tcp(&self, port: u16, host: Option<&str>) -> Result<ShellStream> {
        let transport = self.transport().await?;
        command::host_transport::open_tcp(transport, port, host).await
    }

    /// Starts the built-in `monkey` utility on the device, connects to it and
    /// hands the connection to the [`Monkey`](crate::monkey::Monkey) client.
    pub async fn open_monkey(
        &self,
        port: Option<u16>,
    ) -> Result<crate::monkey::Client<crate::connection::RawStream<tokio::net::TcpStream>>> {
        let port = port.unwrap_or(1080);

        match self.open_tcp(port, None).await {
            Ok(stream) => Ok(crate::monkey::Monkey::connect_stream(stream)),
            Err(err) => {
                crate::debug!("Monkey can't be reached, trying to start it");
                let transport = self.transport().await?;
                let _out = command::host_transport::monkey(transport, port).await?;

                for _ in 0..20 {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    if let Ok(stream) = self.open_tcp(port, None).await {
                        return Ok(crate::monkey::Monkey::connect_stream(stream));
                    }
                }
                Err(err)
            }
        }
    }

    /// Calls the `logcat` utility on the device and hands off the connection to
    /// [`LogcatReader`], a pure Rust Logcat client. Analogous to
    /// `adb logcat -B`.
    pub async fn open_logcat(
        &self,
        clear: bool,
    ) -> Result<LogcatReader<LineTransform<crate::connection::RawStream<tokio::net::TcpStream>>>>
    {
        let transport = self.transport().await?;
        let stream = command::host_transport::logcat(transport, clear).await?;
        Ok(LogcatReader::connect(stream))
    }

    /// Tracks `/proc/stat` and emits useful information, such as CPU load.
    pub async fn open_proc_stat(&self) -> Result<ProcStat<tokio::net::TcpStream>> {
        let sync = self.sync_service().await?;
        Ok(ProcStat::new(sync))
    }

    /// Deletes all data associated with a package from the device. Roughly
    /// analogous to `adb shell pm clear <pkg>`.
    pub async fn clear(&self, pkg: &str) -> Result<bool> {
        let mut transport = self.transport().await?;
        command::host_transport::clear(&mut transport, pkg).await
    }

    /// Installs the APK on the device, replacing any previously installed
    /// version. Roughly analogous to `adb install -r <apk>`.
    pub async fn install(&self, apk: &str) -> Result<bool> {
        let temp = crate::sync::temp(apk);
        self.push(PushSource::file(apk), &temp, None, PushOptions::default())
            .await?;
        self.install_remote(&temp).await
    }

    /// Installs the APK from a stream, replacing any previously installed
    /// version.
    pub async fn install_stream<R: AsyncRead + Unpin + Send + 'static>(
        &self,
        apk: R,
    ) -> Result<bool> {
        let temp = crate::sync::temp("_stream.apk");
        self.push(PushSource::from(apk), &temp, None, PushOptions::default())
            .await?;
        self.install_remote(&temp).await
    }

    /// Installs an APK file which must already be located on the device file
    /// system. The file is removed when the command completes.
    pub async fn install_remote(&self, apk: &str) -> Result<bool> {
        let mut transport = self.transport().await?;
        let result = command::host_transport::install(&mut transport, apk).await;
        let _ = self
            .shell_args(&[Arg::from("rm"), Arg::from("-f"), Arg::from(apk)])
            .await;
        result.map(|_| true)
    }

    /// Uninstalls the package from the device. Roughly analogous to
    /// `adb uninstall <pkg>`.
    pub async fn uninstall(&self, pkg: &str) -> Result<bool> {
        let mut transport = self.transport().await?;
        command::host_transport::uninstall(&mut transport, pkg).await
    }

    /// Tells you if the specific package is installed or not.
    pub async fn is_installed(&self, pkg: &str) -> Result<bool> {
        let mut transport = self.transport().await?;
        command::host_transport::is_installed(&mut transport, pkg).await
    }

    /// Starts the configured activity on the device. Roughly analogous to
    /// `adb shell am start <options>`.
    pub async fn start_activity(&self, options: StartActivityOptions) -> Result<bool> {
        let mut options = options;
        loop {
            let mut transport = self.transport().await?;
            match command::host_transport::start_activity(&mut transport, &options).await {
                Err(err) if err.is_no_user_option() => {
                    options.service.user = None;
                    continue;
                }
                other => return other,
            }
        }
    }

    /// Starts the configured service on the device. Roughly analogous to
    /// `adb shell am startservice <options>`.
    pub async fn start_service(&self, mut options: StartServiceOptions) -> Result<bool> {
        if options.user.is_none() {
            options.user = Some(0);
        }
        loop {
            let mut transport = self.transport().await?;
            match command::host_transport::start_service(&mut transport, &options).await {
                Err(err) if err.is_no_user_option() => {
                    options.user = None;
                    continue;
                }
                other => return other,
            }
        }
    }

    /// Establishes a new sync connection that can be used to push and pull
    /// files.
    pub async fn sync_service(&self) -> Result<Sync<tokio::net::TcpStream>> {
        let transport = self.transport().await?;
        command::host_transport::sync_service(transport).await
    }

    /// Retrieves information about the given path.
    pub async fn stat(&self, path: &str) -> Result<Stats> {
        let mut sync = self.sync_service().await?;
        let stat = sync.stat(path).await;
        sync.end().await;
        stat
    }

    /// Lists the contents of a directory.
    pub async fn readdir(&self, path: &str) -> Result<Vec<Entry>> {
        let mut sync = self.sync_service().await?;
        let entries = sync.readdir(path).await;
        sync.end().await;
        entries
    }

    /// Pulls a file from the device.
    ///
    /// The connection cannot be reused, but it will be closed automatically
    /// when the transfer is dropped.
    pub async fn pull(
        &self,
        path: &str,
        options: PullOptions,
    ) -> Result<PullTransfer<'static, tokio::net::TcpStream>> {
        let sync = self.sync_service().await?;
        sync.pull_owned(path, options).await
    }

    /// Pushes a file to the device.
    pub async fn push(
        &self,
        contents: PushSource,
        path: &str,
        mode: Option<u32>,
        options: PushOptions,
    ) -> Result<u64> {
        let mut sync = self.sync_service().await?;
        let result = match contents {
            PushSource::File(file) => sync.push_file(&file, path, mode, options).await,
            PushSource::Stream(stream) => sync.push(stream, path, mode, options).await,
        };
        sync.end().await;
        result
    }

    /// Puts the device's ADB daemon into tcp mode, allowing you to use
    /// `Client::connect()` to connect to it. Same as `adb tcpip <port>`.
    pub async fn tcpip(&self, port: Option<u16>) -> Result<u16> {
        let mut transport = self.transport().await?;
        command::host_transport::tcpip(&mut transport, port.unwrap_or(5555)).await
    }

    /// Puts the device's ADB daemon back into USB mode. Reverses
    /// [`DeviceClient::tcpip`]. Same as `adb usb`.
    pub async fn usb(&self) -> Result<bool> {
        let mut transport = self.transport().await?;
        command::host_transport::usb(&mut transport).await
    }

    /// Waits until the device has finished booting.
    pub async fn wait_boot_complete(&self) -> Result<bool> {
        let mut transport = self.transport().await?;
        command::host_transport::wait_boot_complete(&mut transport).await
    }

    /// Waits until ADB can see the device.
    pub async fn wait_for_device(&self) -> Result<String> {
        let mut connection = self.connection().await?;
        command::host_serial::wait_for_device(&mut connection, &self.serial).await
    }

    /// Reattaches ADB to the device's ADB USB interface.
    pub async fn attach(&self) -> Result<bool> {
        let mut connection = self.connection().await?;
        command::host_serial::attach(&mut connection, &self.serial).await
    }

    /// Detaches ADB's USB interface from ADB.
    pub async fn detach(&self) -> Result<bool> {
        let mut connection = self.connection().await?;
        command::host_serial::detach(&mut connection, &self.serial).await
    }
}

impl std::fmt::Debug for DeviceClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceClient")
            .field("serial", &self.serial)
            .finish_non_exhaustive()
    }
}

/// Convenience helpers for building [`StartActivityOptions`].
impl StartActivityOptions {
    /// Creates options for starting an activity with the given component.
    pub fn component(component: impl Into<String>) -> Self {
        let mut options = StartActivityOptions::default();
        options.service.component = Some(component.into());
        options
    }

    /// Creates options for starting an activity with the given action.
    pub fn action(action: impl Into<String>) -> Self {
        let mut options = StartActivityOptions::default();
        options.service.action = Some(action.into());
        options
    }

    /// Sets the extras.
    pub fn with_extras(mut self, extras: Extras) -> Self {
        self.service.extras = Some(extras);
        self
    }

    /// Sets the user.
    pub fn with_user(mut self, user: u32) -> Self {
        self.service.user = Some(user);
        self
    }
}

/// Convenience helpers for building [`StartServiceOptions`].
impl StartServiceOptions {
    /// Creates options for starting a service with the given component.
    pub fn component(component: impl Into<String>) -> Self {
        StartServiceOptions {
            component: Some(component.into()),
            ..Default::default()
        }
    }

    /// Creates options for starting a service with the given action.
    pub fn action(action: impl Into<String>) -> Self {
        StartServiceOptions {
            action: Some(action.into()),
            ..Default::default()
        }
    }

    /// Sets the extras.
    pub fn with_extras(mut self, extras: Extras) -> Self {
        self.extras = Some(extras);
        self
    }

    /// Sets the user.
    pub fn with_user(mut self, user: u32) -> Self {
        self.user = Some(user);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_start_activity_builder() {
        use crate::types::ExtraValue;

        let options = StartActivityOptions::action("android.intent.action.VIEW")
            .with_extras(Extras::Map(vec![
                ("key".to_string(), ExtraValue::String("value".into())),
                ("count".to_string(), ExtraValue::Int(42)),
            ]))
            .with_user(10);
        assert_eq!(
            options.service.action.as_deref(),
            Some("android.intent.action.VIEW")
        );
        assert_eq!(options.service.user, Some(10));
        match options.service.extras {
            Some(Extras::Map(map)) => assert_eq!(map.len(), 2),
            _ => panic!("expected map extras"),
        }
    }
}
