# git-multi-repo
`git-multi-repo` is a git plugin written in python for issuing commands to multiple git repositories in a directory.

# Usage
By default `git-multi-repo` iterates over all git repositories in the working directory and executes the specified git command:

    $> git multi-repo fetch

## Repository Dictionary File
A repo dictionary file (`.git-multi-repo`) can be created or updated with the `git multi-repo --sync` option in a working directory with valid repositories.

The dictionary file has the following example content:

    {
      "dir-0": "https://my-git-repo-0"
      "dir-1": "https://my-git-repo-1"
    }

## Checking Repositories
With the `git multi-repo --check` option the repo dictionary file can be checked against the local repositories.<br>Missing repositories are reported as warnings.

## Omitting Repositories
With the `git multi-repo --omit <repository-list>` option a list of repositories can be omitted.<br>The `<repository-list>` needs to be comma separated (e.g. repo0,repo1).

## Clone Repositories
The git clone command is captured by `git-multi-repo` and used to clone all repositories specified in the repo dictionary file (`.git-multi-repo`).

# Install
Copy `git-multi-repo` to a directory included in your system PATH and make it executable.

# Help
Use the help function for more information:

    $> git multi-repo -h/--help

# License
This software is available under the MIT license.
