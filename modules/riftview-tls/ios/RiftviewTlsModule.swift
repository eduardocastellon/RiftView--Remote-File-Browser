import CryptoKit
import ExpoModulesCore
import Foundation
import Security

// See modules/riftview-tls/index.ts and client_reqs.md §4.

/// Fixed DER header that turns a raw 65-byte P-256 point into SubjectPublicKeyInfo.
private let p256SpkiHeader = Data([
  0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01,
  0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
])

/// SHA-256 of the leaf certificate's SubjectPublicKeyInfo.
private func spkiHash(_ trust: SecTrust) -> Data? {
  guard
    let chain = SecTrustCopyCertificateChain(trust) as? [SecCertificate],
    let leaf = chain.first,
    let key = SecCertificateCopyKey(leaf),
    let raw = SecKeyCopyExternalRepresentation(key, nil) as Data?
  else { return nil }
  // iOS returns the raw EC point (04 || X || Y), not SPKI. The host uses P-256.
  guard raw.count == 65, raw.first == 0x04 else { return nil }
  return Data(SHA256.hash(data: p256SpkiHeader + raw))
}

/// expectedPin == nil: probe mode, accept any certificate and record its hash.
/// expectedPin != nil: accept only that hash.
private final class PinningDelegate: NSObject, URLSessionDelegate {
  let expectedPin: Data?
  private(set) var seenPin: Data?
  private(set) var pinMismatch = false

  init(expectedPin: Data?) {
    self.expectedPin = expectedPin
  }

  func urlSession(
    _ session: URLSession,
    didReceive challenge: URLAuthenticationChallenge,
    completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void
  ) {
    guard
      challenge.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust,
      let trust = challenge.protectionSpace.serverTrust
    else {
      completionHandler(.cancelAuthenticationChallenge, nil)
      return
    }
    guard let pin = spkiHash(trust) else {
      completionHandler(.cancelAuthenticationChallenge, nil)
      return
    }
    seenPin = pin
    if let expected = expectedPin, !constantTimeEqual(expected, pin) {
      pinMismatch = true
      completionHandler(.cancelAuthenticationChallenge, nil)
      return
    }
    // Pin (or probe) replaces CA and hostname validation: we connect by IP.
    completionHandler(.useCredential, URLCredential(trust: trust))
  }
}

private func constantTimeEqual(_ a: Data, _ b: Data) -> Bool {
  guard a.count == b.count else { return false }
  var diff: UInt8 = 0
  for i in 0..<a.count { diff |= a[a.startIndex + i] ^ b[b.startIndex + i] }
  return diff == 0
}

private func makeSession(_ delegate: PinningDelegate, timeoutMs: Double) -> URLSession {
  let config = URLSessionConfiguration.ephemeral
  config.timeoutIntervalForRequest = timeoutMs / 1000
  config.requestCachePolicy = .reloadIgnoringLocalCacheData
  config.urlCache = nil
  config.httpCookieStorage = nil
  return URLSession(configuration: config, delegate: delegate, delegateQueue: nil)
}

private func pinMismatchError() -> Exception {
  Exception(
    name: "PinMismatch",
    description: "The host's certificate doesn't match the one saved at pairing. Its identity changed.",
    code: "ERR_PIN_MISMATCH"
  )
}

private func requestFailedError(_ error: Error?) -> Exception {
  Exception(
    name: "RequestFailed",
    description: error?.localizedDescription ?? "Couldn't reach the host.",
    code: "ERR_REQUEST_FAILED"
  )
}

public class RiftviewTlsModule: Module {
  public func definition() -> ModuleDefinition {
    Name("RiftviewTls")

    AsyncFunction("probePin") { (host: String, port: Int, timeoutMs: Double, promise: Promise) in
      guard let url = URL(string: "https://\(host):\(port)/api/health") else {
        promise.reject(requestFailedError(nil))
        return
      }
      let delegate = PinningDelegate(expectedPin: nil)
      let session = makeSession(delegate, timeoutMs: timeoutMs)
      session.dataTask(with: url) { _, _, error in
        defer { session.finishTasksAndInvalidate() }
        // The pin is captured during the TLS handshake, so it's available even
        // if the HTTP part failed afterwards.
        if let pin = delegate.seenPin {
          promise.resolve(pin.base64EncodedString())
        } else {
          promise.reject(requestFailedError(error))
        }
      }.resume()
    }

    AsyncFunction("request") {
      (method: String, urlString: String, headers: [String: String], body: String?, pin: String,
       timeoutMs: Double, promise: Promise) in
      guard let url = URL(string: urlString), let expected = Data(base64Encoded: pin), expected.count == 32 else {
        promise.reject(requestFailedError(nil))
        return
      }
      var request = URLRequest(url: url)
      request.httpMethod = method
      for (name, value) in headers { request.setValue(value, forHTTPHeaderField: name) }
      if let body { request.httpBody = body.data(using: .utf8) }

      let delegate = PinningDelegate(expectedPin: expected)
      let session = makeSession(delegate, timeoutMs: timeoutMs)
      session.dataTask(with: request) { data, response, error in
        defer { session.finishTasksAndInvalidate() }
        if delegate.pinMismatch {
          promise.reject(pinMismatchError())
          return
        }
        guard let http = response as? HTTPURLResponse else {
          promise.reject(requestFailedError(error))
          return
        }
        var outHeaders: [String: String] = [:]
        for (key, value) in http.allHeaderFields {
          outHeaders[String(describing: key).lowercased()] = String(describing: value)
        }
        promise.resolve([
          "status": http.statusCode,
          "headers": outHeaders,
          "body": data.flatMap { String(data: $0, encoding: .utf8) } ?? "",
        ])
      }.resume()
    }
  }
}
