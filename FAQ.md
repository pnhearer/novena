# Questions and answers

Plain answers about what this project is and how you may use it. [LICENSE.md](LICENSE.md) has a table that says which licence fits which use. The full licence texts are in the LICENSES folder, and they are what counts if this page ever reads differently.

## What is this for?

Research and curiosity. It is an attempt to understand how a console's graphics interface behaves and to rebuild it on top of Vulkan, in the open, so that others can learn from it and build on it.

## Can I use it?

Yes. There are three ways, and you choose one:

1. **Noncommercial use is free.** Personal projects, hobby work, study, teaching, research, and use by charities, schools, public bodies and similar organisations.
2. **GPL projects can include it for free.** If your project is under the GPL (version 3, or version 2 "or later"), novena can be part of it under the GPL.
3. **Closed commercial use is paid.** If you want to make money with it and keep your own source closed, you need a commercial licence.

## Can I change it?

Yes. Under the noncommercial licence you can modify it however you like for noncommercial purposes and share your changes on the same terms. Under the GPL you can modify it and share your changes under the GPL. Either way, pass the licence terms and the copyright notice along with it so the next person knows the terms too. Each licence says exactly how.

## Why is it not simply free for everything?

The work is shared so people can learn from it and enjoy it. It is not shared so that someone can take it for nothing, put it in a closed product and sell that to people who have no idea what is inside or who made it. Low-effort products built on other people's unpaid work are bad for the people who buy them and bad for the people who did the work.

So closed commercial use has a price. Anyone who wants to make money with novena without sharing their own source can do so by getting a commercial licence. See [COMMERCIAL.md](COMMERCIAL.md).

## What counts as commercial?

Roughly, anything done to make money or as part of a business. Selling a device or program that includes novena, or bundling it with something you sell, are the clear cases. A hobby project that takes no money is not commercial.

The noncommercial licence also names kinds of organisation that may use it whatever their funding: charities, schools and universities, public research bodies, public safety and health organisations, environmental groups and government institutions. Its text has the exact list.

If you are in between, ask before you ship.

## Why are there two free licences? Is one not enough?

Each covers people the other leaves out. The noncommercial licence is the simple one: its main condition is "not for commercial use". But projects under the GPL are not allowed to take in code that carries a restriction like that, so the GPL is offered as well and those projects can include novena too.

## Can a company use the GPL option and sell it without paying?

Only by following the GPL in full. Whatever they hand to customers that combines novena with their own code has to be under the GPL, and the customers must be able to get its source. For software shipped in consumer products, the GPL in most cases also requires what a buyer needs to install a changed version. A seller who does all that is sharing their work back, which is fair. A seller who wants to keep things closed needs the commercial licence.

One thing the GPL does not cover is worth saying plainly. Someone who only runs novena on their own machines, and never hands the software to anyone else, is not required by the GPL to share source, even if they charge for what those machines do.

## I run a free, community emulator or tool. Can I use it?

Yes. If your project is noncommercial, use the noncommercial licence. If your project is under the GPL, use the GPL. Either way you may bundle novena. The one case that fits neither is a project under GPL version 2 only. If that is you, open an issue.

## Is this open source?

Under the GPL option, yes: the GPL is an open source and free software licence. The noncommercial option is not open source by the usual definition, because that definition does not allow a noncommercial restriction. You only need one of the two, so pick the one that suits you.

## Can I contribute?

Yes, and thank you. Because commercial licences are offered, contributors are asked to agree to a short contributor agreement so the project is allowed to include their work under the two public licences and under commercial licences. See [CONTRIBUTING.md](CONTRIBUTING.md) and [CLA.md](CLA.md). The clean-room rules in [CLEAN-ROOM.md](CLEAN-ROOM.md) apply to every contribution.

## Does it include anything from a console maker or from games?

No. No code, headers, documentation, shaders, keys, firmware or game data from anyone else. It was written from scratch under clean-room rules, and it does not decrypt or unlock anything. See [LEGAL.md](LEGAL.md).

## Will it run my games?

Not by itself, and not yet. It is one piece that an emulator or similar tool would use, and at the moment it does not render anything. The README says where things stand.
