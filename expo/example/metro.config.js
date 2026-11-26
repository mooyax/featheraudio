// the test sounds are bundled as assets
const { getDefaultConfig } = require("expo/metro-config");
const config = getDefaultConfig(__dirname);
for (const ext of ["ogg", "mp3", "mp2", "wav", "mod", "xm", "it", "s3m"]) {
  if (!config.resolver.assetExts.includes(ext)) config.resolver.assetExts.push(ext);
}
module.exports = config;
