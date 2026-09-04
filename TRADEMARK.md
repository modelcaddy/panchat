# Trademark policy

> **Draft.** This states an intent that has not been reviewed by a lawyer, and it makes no claim
> about where any mark is registered. Read it as what this project asks of you, not as a legal
> opinion.

## The short version

The **code** is open. The **names** are not the code.

Everything in this repository is licensed under [Apache-2.0](LICENSE), which lets you use, modify,
distribute and sell it, including inside something closed. That license deliberately does not give
you rights in a name: [section 6](https://www.apache.org/licenses/LICENSE-2.0#trademark) grants no
permission to use the licensor's trade names, trademarks or product names, beyond describing where
the work came from. This file says what we ask on top of that, and it is short because the ask is
small.

The names this concerns are **panchat**, the name of this library, and **ModelCaddy**, the name of
the product it was extracted from.

## What you may do, and we would like you to

- **Use the library in anything**, commercial or not, open or closed. That is what the license is
  for and there is nothing further to ask.
- **Depend on it by name.** `panchat = "0.3"` in a manifest, `use panchat::…` in code, and
  `panchat` on a command line are the name doing its job.
- **Say truthfully what you built on.** "Uses panchat", "built with panchat", "reads exports via
  panchat", "a fork of panchat" — all fine, and no permission is needed for an accurate,
  non-misleading reference.
- **Fork it.** Publicly, permanently, for any reason, including to compete. Hosting a fork under
  the same name while you work on changes or prepare a pull request is fine; what follows is about
  *distributing* one.
- **Write about it**, review it, teach it, or criticise it, using the name to refer to the thing
  you are writing about.

## What we ask you not to do

- **Do not distribute a modified version under these names.** If you fork this library and ship the
  result to other people, give it your own name. Someone installing a package called `panchat`
  should be getting this code, or should be clearly told they are not.
- **Do not name your project after ours.** `panchat-plus`, `ModelCaddy Cloud`, or anything that
  reads as an official edition or an endorsed variant. Pick a name that is yours.
- **Do not use the ModelCaddy name or logo for your own product, service or organisation.** The
  library's name is here to be depended on; the product's name is not.
- **Do not imply endorsement, affiliation or an official relationship** that does not exist. If
  there is a real chance a reader would think your thing is our thing, that is the line.

If you are distributing a fork and want to say where it came from, this works: name your project
whatever you like, and state plainly that it is built from panchat and is not affiliated with or
endorsed by the panchat maintainers.

## Why this exists, given the license

Apache-2.0 is a one-way door: the code released under it stays released, and anyone may build a
business on it. That is intended. A name is the one thing that cannot be shared without making the
open half untrustworthy — if two different programs answer to `panchat`, "your export was read by
panchat" stops meaning anything, and the honesty this project is built on goes with it.

So the licence covers the code, and this file covers only the confusion.

## Not a NOTICE file

There is deliberately no `NOTICE` file in this repository, and this is not one.

Apache-2.0 section 4(d) makes a `NOTICE` file's contents propagate into every derivative work, and
states that those contents are informational and cannot modify the license. Section 6 goes further
in the unhelpful direction: reproducing the content of a `NOTICE` file is one of the two express
exceptions to the trademark non-grant. A trademark restriction placed in `NOTICE` would therefore
be unenforceable as a license term *and* would hand every fork explicit permission to reproduce the
name it was trying to protect.

Section 4(c) is the one that fits. It requires a redistributor to retain trademark and attribution
notices found in the source form, which is exactly what this file is and where it lives.

## Questions

Ask, by opening an issue. A question about whether something is confusing is not an imposition, and
the answer is usually yes, that is fine, go ahead.
