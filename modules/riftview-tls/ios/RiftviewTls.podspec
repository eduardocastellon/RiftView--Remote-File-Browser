Pod::Spec.new do |s|
  s.name           = 'RiftviewTls'
  s.version        = '1.0.0'
  s.summary        = 'Pinned HTTPS for RiftView hosts with self-signed certificates'
  s.description    = 'Pinned HTTPS for RiftView hosts with self-signed certificates'
  s.license        = 'MIT'
  s.author         = 'RiftView'
  s.homepage       = 'https://github.com/eduardocastellon/RiftView--Remote-File-Browser'
  s.platforms      = { :ios => '16.4' }
  s.swift_version  = '5.9'
  s.source         = { git: '' }
  s.static_framework = true

  s.dependency 'ExpoModulesCore'

  s.source_files = "**/*.{h,m,swift}"
  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
    'SWIFT_COMPILATION_MODE' => 'wholemodule'
  }
end
