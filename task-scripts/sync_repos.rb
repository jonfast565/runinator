require 'fiber'
require 'fileutils'
require 'pathname'

def git_repository?(folder)
  File.directory?(File.join(folder, '.git'))
end

def fetch_and_sync_repo(repo_path)
  Dir.chdir(repo_path) do
    puts "Fetching and syncing repository at: #{repo_path}"
    system('git fetch --all')
    system('git pull')
  end
end

def search_and_sync_repositories(root_folder)
  queue = [root_folder]
  fibers = []

  until queue.empty?
    current_folder = queue.shift
    entries = Dir.entries(current_folder).reject { |e| e.start_with?('.') } # Exclude hidden files

    entries.each do |entry|
      path = File.join(current_folder, entry)
      next unless File.directory?(path)

      if git_repository?(path)
        fibers << Fiber.new do
          fetch_and_sync_repo(path)
        end
      else
        queue << path
      end
    end
  end

  fibers.each(&:resume)
end

if ARGV.empty?
  puts 'Usage: ruby sync_git_repos.rb <root_folder>'
  exit(1)
end

root_folder = ARGV[0]

unless Dir.exist?(root_folder)
  puts "Error: Folder '#{root_folder}' does not exist."
  exit(1)
end

puts "Searching for Git repositories in: #{root_folder}"
search_and_sync_repositories(root_folder)
puts "Done!"
