export function textPreview(text,length=240){
  let result='',count=0;
  for(const character of text){if(count++===length)break;result+=character;}
  return result;
}
