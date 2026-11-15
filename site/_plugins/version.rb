# The app's version, for the sidebar's tag, read from the root Cargo.toml's
# [package] table at build time, so a release that bumps it updates the
# manual with no other change. Exposed as `site.omacharts_version`.
module Omacharts
  module Version
    CARGO = File.expand_path("../../Cargo.toml", __dir__)

    def self.read(path = CARGO)
      in_package = false
      File.foreach(path) do |line|
        line = line.strip
        if line.start_with?("[")
          in_package = line == "[package]"
        elsif in_package && (match = line.match(/\Aversion\s*=\s*"([^"]+)"/))
          return match[1]
        end
      end
      nil
    rescue Errno::ENOENT
      nil
    end
  end
end

Jekyll::Hooks.register :site, :post_read do |site|
  site.config["omacharts_version"] = Omacharts::Version.read
end
