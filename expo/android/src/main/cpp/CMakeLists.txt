# featheraudio for Expo (Android): the JNI bridge (featheraudio_jni.c) linked with the
# Rust static library android/libs/<ABI>/libfeatheraudio.a (built by expo/build-mobile.ps1).
cmake_minimum_required(VERSION 3.13)
project(featheraudio_jni C)

set(FA_LIB ${CMAKE_CURRENT_SOURCE_DIR}/../../../libs/${ANDROID_ABI}/libfeatheraudio.a)
if(NOT EXISTS ${FA_LIB})
  message(FATAL_ERROR "libfeatheraudio.a for ${ANDROID_ABI} not found: run expo/build-mobile.ps1 in featheraudio and copy the module again")
endif()

add_library(featheraudio STATIC IMPORTED)
set_target_properties(featheraudio PROPERTIES IMPORTED_LOCATION ${FA_LIB})

add_library(featheraudio_jni SHARED featheraudio_jni.c)
target_include_directories(featheraudio_jni PRIVATE ${CMAKE_CURRENT_SOURCE_DIR})
# dl / m are used by the Rust standard library, log for messages
target_link_libraries(featheraudio_jni featheraudio log dl m)
# Google Play requires 16 KB page alignment (Android 15+ devices with 16 KB pages);
# --gc-sections drops the Rust code the bridge never calls
target_link_options(featheraudio_jni PRIVATE "-Wl,-z,max-page-size=16384" "-Wl,-z,common-page-size=16384" "-Wl,--gc-sections")
