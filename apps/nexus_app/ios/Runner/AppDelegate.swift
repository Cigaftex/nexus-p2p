import Flutter
import UIKit
import Darwin
import CoreBluetooth

@main
@objc class AppDelegate: FlutterAppDelegate, FlutterImplicitEngineDelegate {
  private var bonjourAdapter: BonjourTransportAdapter?

  override func application(
    _ application: UIApplication,
    didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
  ) -> Bool {
    return super.application(application, didFinishLaunchingWithOptions: launchOptions)
  }

  func didInitializeImplicitFlutterEngine(_ engineBridge: FlutterImplicitEngineBridge) {
    GeneratedPluginRegistrant.register(with: engineBridge.pluginRegistry)
    guard let registrar = engineBridge.pluginRegistry.registrar(forPlugin: "NexusBonjourTransport") else {
      return
    }
    let channel = FlutterMethodChannel(
      name: "app.nexus/transport",
      binaryMessenger: registrar.messenger()
    )
    bonjourAdapter = BonjourTransportAdapter(channel: channel)
  }
}

private final class BonjourTransportAdapter: NSObject, NetServiceBrowserDelegate, NetServiceDelegate, CBCentralManagerDelegate, CBPeripheralManagerDelegate {
  private static let proximityService = CBUUID(string: "8B204787-8D3A-4B31-9B6C-7A2253D84250")
  private let channel: FlutterMethodChannel
  private var browser: NetServiceBrowser?
  private var publisher: NetService?
  private var services: [ObjectIdentifier: NetService] = [:]
  private var peerIds: [ObjectIdentifier: String] = [:]
  private var localDeviceId = ""
  private var central: CBCentralManager?
  private var peripheral: CBPeripheralManager?

  init(channel: FlutterMethodChannel) {
    self.channel = channel
    super.init()
    channel.setMethodCallHandler { [weak self] call, result in
      guard let self else { return result(FlutterError(code: "unavailable", message: "Bonjour adapter unavailable", details: nil)) }
      switch call.method {
      case "startBonjour":
        guard let arguments = call.arguments as? [String: Any],
              let deviceId = arguments["deviceId"] as? String,
              let displayName = arguments["displayName"] as? String,
              let signingKey = arguments["signingPublicKey"] as? String,
              let exchangeKey = arguments["exchangePublicKey"] as? String,
              let version = arguments["version"] as? Int,
              let port = arguments["port"] as? Int else {
          return result(FlutterError(code: "arguments", message: "Invalid Bonjour configuration", details: nil))
        }
        self.start(
          deviceId: deviceId,
          displayName: displayName,
          signingKey: signingKey,
          exchangeKey: exchangeKey,
          version: version,
          port: port
        )
        result(nil)
      case "stopBonjour":
        self.stop()
        result(nil)
      default:
        result(FlutterMethodNotImplemented)
      }
    }
  }

  private func start(
    deviceId: String,
    displayName: String,
    signingKey: String,
    exchangeKey: String,
    version: Int,
    port: Int
  ) {
    stop()
    localDeviceId = deviceId
    if central == nil {
      central = CBCentralManager(delegate: self, queue: .main)
      peripheral = CBPeripheralManager(delegate: self, queue: .main)
    }

    let service = NetService(
      domain: "local.",
      type: "_nexus-p2p._tcp.",
      name: "nexus-\(deviceId.prefix(8))",
      port: Int32(port)
    )
    service.includesPeerToPeer = true
    service.delegate = self
    let properties = [
      "id": deviceId,
      "name": displayName,
      "sign": signingKey,
      "exchange": exchangeKey,
      "v": String(version),
    ].mapValues { Data($0.utf8) }
    service.setTXTRecord(NetService.data(fromTXTRecord: properties))
    service.publish(options: [.noAutoRename])
    publisher = service

    let browser = NetServiceBrowser()
    browser.includesPeerToPeer = true
    browser.delegate = self
    browser.searchForServices(ofType: "_nexus-p2p._tcp.", inDomain: "local.")
    self.browser = browser
    startBluetooth()
  }

  private func stop() {
    browser?.stop()
    publisher?.stop()
    services.values.forEach { $0.stop() }
    browser = nil
    publisher = nil
    services.removeAll()
    peerIds.removeAll()
    central?.stopScan()
    peripheral?.stopAdvertising()
  }

  func netServiceBrowser(
    _ browser: NetServiceBrowser,
    didFind service: NetService,
    moreComing: Bool
  ) {
    let key = ObjectIdentifier(service)
    services[key] = service
    service.delegate = self
    service.resolve(withTimeout: 8)
  }

  func netServiceBrowser(
    _ browser: NetServiceBrowser,
    didRemove service: NetService,
    moreComing: Bool
  ) {
    let key = ObjectIdentifier(service)
    if let peerId = peerIds[key] {
      channel.invokeMethod("bonjourPeerLost", arguments: ["deviceId": peerId])
    }
    services.removeValue(forKey: key)
    peerIds.removeValue(forKey: key)
  }

  func netServiceDidResolveAddress(_ sender: NetService) {
    guard let txtData = sender.txtRecordData() else { return }
    let txt = NetService.dictionary(fromTXTRecord: txtData).compactMapValues {
      String(data: $0, encoding: .utf8)
    }
    guard let deviceId = txt["id"],
          deviceId != localDeviceId,
          let displayName = txt["name"],
          let signingKey = txt["sign"],
          let exchangeKey = txt["exchange"],
          let host = bestHost(from: sender.addresses ?? []),
          sender.port > 0 else { return }

    let key = ObjectIdentifier(sender)
    peerIds[key] = deviceId
    channel.invokeMethod("bonjourPeer", arguments: [
      "identity": [
        "device_id": deviceId,
        "display_name": displayName,
        "signing_public_key": signingKey,
        "exchange_public_key": exchangeKey,
      ],
      "host": host,
      "port": sender.port,
    ])
  }

  private func bestHost(from addresses: [Data]) -> String? {
    let candidates = addresses.compactMap(numericHost)
    return candidates.min { rank($0) < rank($1) }?.host
  }

  private func rank(_ candidate: (family: Int32, host: String)) -> Int {
    if candidate.family == AF_INET && !candidate.host.hasPrefix("169.254.") && candidate.host != "127.0.0.1" {
      return 0
    }
    if candidate.family == AF_INET && candidate.host != "127.0.0.1" {
      return 1
    }
    if candidate.family == AF_INET6 && !candidate.host.hasPrefix("fe80:") && candidate.host != "::1" {
      return 2
    }
    return 3
  }

  private func numericHost(from data: Data) -> (family: Int32, host: String)? {
    data.withUnsafeBytes { bytes in
      guard let baseAddress = bytes.baseAddress else { return nil }
      let address = baseAddress.assumingMemoryBound(to: sockaddr.self)
      var host = [CChar](repeating: 0, count: Int(NI_MAXHOST))
      let status = getnameinfo(
        address,
        socklen_t(data.count),
        &host,
        socklen_t(host.count),
        nil,
        0,
        NI_NUMERICHOST
      )
      guard status == 0 else { return nil }
      return (Int32(address.pointee.sa_family), String(cString: host))
    }
  }

  func centralManagerDidUpdateState(_ central: CBCentralManager) {
    startBluetooth()
  }

  func peripheralManagerDidUpdateState(_ peripheral: CBPeripheralManager) {
    startBluetooth()
  }

  private func startBluetooth() {
    guard !localDeviceId.isEmpty else { return }
    if central?.state == .poweredOn {
      central?.stopScan()
      central?.scanForPeripherals(
        withServices: [Self.proximityService],
        options: [CBCentralManagerScanOptionAllowDuplicatesKey: true]
      )
    }
    if peripheral?.state == .poweredOn {
      peripheral?.stopAdvertising()
      peripheral?.startAdvertising([
        CBAdvertisementDataServiceUUIDsKey: [Self.proximityService],
        CBAdvertisementDataLocalNameKey: "NX\(localDeviceId.prefix(8))",
      ])
    }
  }

  func centralManager(
    _ central: CBCentralManager,
    didDiscover peripheral: CBPeripheral,
    advertisementData: [String: Any],
    rssi RSSI: NSNumber
  ) {
    guard RSSI.intValue != 127 else { return }
    let name = (advertisementData[CBAdvertisementDataLocalNameKey] as? String) ?? peripheral.name
    guard let name, name.hasPrefix("NX"), name.count >= 10 else { return }
    let prefix = String(name.dropFirst(2).prefix(8)).lowercased()
    guard prefix != localDeviceId.prefix(8).lowercased() else { return }
    channel.invokeMethod("bleProximity", arguments: [
      "deviceIdPrefix": prefix,
      "rssi": RSSI.intValue,
    ])
  }
}
