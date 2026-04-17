- Seasons in its own folder like S01
- file Title should includ ethe number S01E01.mkv
- Title of the series always in the original language 
	- imdbid optional like:  [imdbid-tt7587890] 
  
## Example 
The Rookie [imdbid-tt7587890]/S03/S03E01 - Consequences WEBRip-1080p.mkv

## download.sh
each season folder (e.g. the chair company/S01) should have one file with 1 line with wget per file. e.g. 
	wget S01E01.mkv
	wget S01E02.mkv
	etc
