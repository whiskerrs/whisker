Pod::Spec.new do |s|
  s.name             = 'LyraRuntime'
  s.version          = '0.1.0'
  s.summary          = 'iOS runtime for the Lyra cross-platform UI framework.'
  s.description      = <<-DESC
    Hosts the Lynx engine and bridges it to the Rust runtime that powers
    Lyra apps. Generated apps depend on this pod via CNG.
  DESC
  s.homepage         = 'https://github.com/itome/lyra'
  s.license          = { :type => 'MIT OR Apache-2.0' }
  s.author           = { 'itome' => 'dev@itome.team' }
  s.source           = { :git => 'https://github.com/itome/lyra.git', :tag => s.version.to_s }

  s.ios.deployment_target = '13.0'
  s.swift_version = '5.9'

  s.source_files = 'Sources/LyraRuntime/**/*.{swift,h,m,mm}'

  s.dependency 'Lynx', '3.7.0'
  s.dependency 'PrimJS', '3.7.0'
end
