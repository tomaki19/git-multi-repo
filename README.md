# git-multi-repo
Git plugin to control multiple git repositories in a directory.

# usage
By default `git-multi-repo` iterates over all git repositories in the working directory and executes the specified git command:

    $> git-multi-repo fetch

## repo dictionary file
A repo dictionary file (`.git-multi-repo`) can be created in the working directory with a list of valid repositories.

The dictionary file has the following example content:

    {
      "dir-0": "https://my-git-repo-0"
      "dir-1": "https://my-git-repo-1"
    }

The repo dictionary file can also created or updated automatically with the `git-multi-repo --update` parameter.

With the `git-multi-repo --file <file>` parameter a other repo dictionary file can be specified for issued command.

## omitting repositories
With the `git-multi-repo --omit <repo-list>` parameter a list of repsitories can be omitted. The `<repo-list>` needs to be comma separated (e.g. repo0,repo1).
