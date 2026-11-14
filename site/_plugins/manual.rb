# The manual's sections, from `manual.sections` in _config.yml. Two things
# the theme cannot do on its own:
#
# - A page per section at /manual/<key>/, so the sidebar's section labels
#   have somewhere to go. Nothing is written under manual/ for them; the
#   label and the description in the config are the whole page, laid out by
#   _layouts/section.html over the chapters that name the section.
#
# - The headings of a chapter, for the contents at the top of it and for
#   its part's page, read off the rendered HTML, so the ids are the ones kramdown actually gave the
#   headings rather than a guess at its slug rules.
module Omacharts
  module Manual
    HEADING = %r{<h([1-6])\b[^>]*\bid="([^"]+)"[^>]*>(.*?)</h\1>}m

    # The headings between two levels in rendered HTML, in order, as
    # {"level", "id", "text"}. Inline code stays code, since half the
    # reference headings name a command; any other tag is dropped.
    def self.headings(html, min, max)
      html.to_s.scan(HEADING).filter_map do |level, id, inner|
        level = level.to_i
        next if level < min || level > max
        # A chapter's see-also is a way out, not part of what it covers.
        next if id == "see-also"

        text = inner.gsub(%r{</?(?!code\b)[a-z][^>]*>}i, "").strip
        { "level" => level, "id" => id, "text" => text }
      end
    end

    # The same headings as nested lists: a heading's children are the
    # deeper headings up to the next one at its level or above. Levels may
    # skip (an h4 straight under an h2), which just nests one step.
    def self.tree(html, min, max, list_class = nil, base = "")
      items = headings(html, min, max)
      return "" if items.empty?

      out = +(list_class ? %(<ul class="#{list_class}">) : "<ul>")
      open = [items.first["level"]] # the level of each list still open
      items.each_with_index do |heading, i|
        level = heading["level"]
        if level > open.last
          out << "<ul>"
          open << level
        else
          out << "</li>" if i.positive?
          while open.size > 1 && level < open.last
            out << "</ul></li>"
            open.pop
          end
        end
        out << %(<li><a href="#{base}##{heading["id"]}">#{heading["text"]}</a>)
      end
      out << "</li>"
      (open.size - 1).times { out << "</ul></li>" }
      out << "</ul>"
    end

    module Filters
      # {{ content | headings_of: 2, 5 }} — the array, for counting.
      def headings_of(html, min = 2, max = 5)
        Manual.headings(html, min.to_i, max.to_i)
      end

      # {{ content | toc_tree: 2, 5, "chapter-toc-list" }} — the nested list.
      def toc_tree(html, min = 2, max = 5, list_class = "toc")
        Manual.tree(html, min.to_i, max.to_i, list_class)
      end

      # {{ chapter | chapter_tree: 2, 3 }} — another chapter's headings as
      # the same nested list, each linking into that chapter, for a part's
      # page. Its markdown goes through the site's converter so the ids are
      # the chapter's own; Liquid in it is not run, and a heading has none.
      def chapter_tree(page, min = 2, max = 3)
        site = @context.registers[:site]
        content = page.respond_to?(:content) ? page.content : page["content"]
        html = site.find_converter_instance(Jekyll::Converters::Markdown).convert(content.to_s)
        url = page.respond_to?(:url) ? page.url : page["url"]
        Manual.tree(html, min.to_i, max.to_i, nil, site.baseurl.to_s + url)
      end
    end

    class SectionPages < Jekyll::Generator
      safe true
      priority :high

      def generate(site)
        Array(site.config.dig("manual", "sections")).each do |section|
          key = section["key"]
          page = Jekyll::PageWithoutAFile.new(site, site.source, "manual", "#{key}.md")
          page.content = ""
          page.data.merge!(
            "layout" => "section",
            "title" => section["label"],
            "description" => section["description"],
            "section_key" => key,
            "badge" => section["badge"],
            "permalink" => "/manual/#{key}/",
            # Not a chapter: out of the sidebar's lists, the reading order
            # and the search, which should find chapters, not their index.
            "nav_exclude" => true,
            "search_exclude" => true
          )
          site.pages << page
        end
      end
    end

    # The manual as a tree, worked out once so the sidebar, the trail above
    # a title, the neighbours below the text and a page's index of what is
    # under it all agree. A page's key is its path under /manual/
    # ("indicators/volume"); a page under another names that key in
    # `parent:`, at any depth. Each manual page gets:
    #
    #   manual_key       its key
    #   manual_children  the pages under it, in nav_order, as {url, title,
    #                    description, family, children}, children nested
    #   manual_trail     the pages above it, outermost first, as {url, title}
    #   manual_ancestors the urls of those, for marking the sidebar open
    #   manual_prev, manual_next  its neighbours in reading order, {url, title}
    #
    # A section's page gets manual_children too: its chapters, nested.
    class Tree < Jekyll::Generator
      safe true
      priority :low

      def generate(site)
        pages = site.pages.select do |page|
          page.url.start_with?("/manual/") && page.data["title"] && !page.data["nav_exclude"]
        end
        key = ->(page) { page.url.delete_prefix("/manual/").delete_suffix("/") }
        by_key = pages.to_h { |page| [key[page], page] }
        under = pages.group_by { |page| page.data["parent"] }
        ordered = ->(list) { Array(list).sort_by { |page| [page.data["nav_order"] || Float::INFINITY, page.data["title"]] } }
        entry = lambda do |page|
          {
            "url" => page.url, "title" => page.data["title"],
            "description" => page.data["description"], "family" => page.data["family"],
            "children" => ordered[under[key[page]]].map(&entry)
          }
        end

        pages.each do |page|
          page.data["manual_key"] = key[page]
          page.data["manual_children"] = ordered[under[key[page]]].map(&entry)
          trail = []
          up = page.data["parent"]
          while up && (above = by_key[up]) && trail.size < 8
            trail.unshift({ "url" => above.url, "title" => above.data["title"] })
            up = above.data["parent"]
          end
          page.data["manual_trail"] = trail
          page.data["manual_ancestors"] = trail.map { |crumb| crumb["url"] }
        end

        # Reading order: the letter, then each section's chapters, each
        # followed depth first by the pages under it.
        walk = ->(page) { [page] + ordered[under[key[page]]].flat_map(&walk) }
        reading = ordered[pages.select { |page| page.data["section"].nil? && page.data["parent"].nil? }]
        Array(site.config.dig("manual", "sections")).each do |section|
          chapters = ordered[pages.select { |page| page.data["section"] == section["key"] && page.data["parent"].nil? }]
          site.pages.find { |page| page.data["section_key"] == section["key"] }&.data&.[]=("manual_children", chapters.map(&entry))
          reading.concat(chapters.flat_map(&walk))
        end
        reading.each_with_index do |page, i|
          neighbour = ->(other) { other && { "url" => other.url, "title" => other.data["title"] } }
          page.data["manual_prev"] = neighbour[i.positive? ? reading[i - 1] : nil]
          page.data["manual_next"] = neighbour[reading[i + 1]]
        end
      end
    end
  end
end

Liquid::Template.register_filter(Omacharts::Manual::Filters)
