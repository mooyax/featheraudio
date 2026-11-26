# featheraudio for Expo (iOS). The engine is the Rust static library in
# ios/FeatherAudioCore.xcframework (built by expo/build-mobile.ps1); Swift calls its C API
# (ios/featheraudio.h) and feeds AVAudioEngine.
Pod::Spec.new do |s|
  s.name           = 'FeatherAudio'
  s.version        = '0.1.0'
  s.summary        = 'featheraudio game audio engine for Expo apps'
  s.license        = { :type => '(MIT OR Apache-2.0) AND BSD-3-Clause' }
  s.author         = 'mooyax'
  s.homepage       = 'https://github.com/mooyax/featheraudio'
  s.platforms      = { :ios => '15.1' }
  s.swift_version  = '5.9'
  s.source         = { git: '' }
  s.static_framework = true

  s.dependency 'ExpoModulesCore'

  s.source_files = 'ios/**/*.{h,swift}'
  s.public_header_files = 'ios/featheraudio.h'
  s.preserve_paths = 'ios/FeatherAudioCore.xcframework'
  s.vendored_frameworks = 'ios/FeatherAudioCore.xcframework'
  s.frameworks = 'AVFoundation'

  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
    'SWIFT_COMPILATION_MODE' => 'wholemodule',
  }
end
